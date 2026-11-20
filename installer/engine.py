"""Prepare and run an isolated SoundTime Compose project on the local Docker engine."""

from __future__ import annotations

import contextlib
import errno
import hashlib
import http.client
import json
import os
import platform
import re
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path, PurePosixPath
from urllib.parse import urlsplit


ROOT = Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent.parent))
ASSETS = {"compose.yaml": "docker-compose.yml", "docker/nginx.conf": "docker/nginx.conf"}
LOCK = ".soundtime-installer.lock"
HEADER = "# SoundTime Installer v1 "
MANIFEST = ".env"
MIN_COMPOSE = (2, 24, 4)


def defaults():
    return dict(directory=str(Path.home() / "SoundTime Server"), port=8880,
                access="local", p2p=False, peerPort=11204, domain="localhost", musicDirectory="",
                storage={"kind": "local"})


def _storage(value):
    if value == {"kind": "local"}:
        return {"kind": "local"}
    reuse = isinstance(value, dict) and value.get("reuseCredentials") is True
    fields = {"kind", "endpoint", "region", "bucket", "prefix"}
    fields.update({"reuseCredentials"} if reuse else {"accessKey", "secretKey"})
    if not isinstance(value, dict) or value.keys() != fields or value.get("kind") != "s3":
        raise ValueError("Choose disk storage or enter your S3 settings.")
    if any(not isinstance(v, str) or len(v) > 2048 or not all(ch.isprintable() for ch in v)
           for k, v in value.items() if k != "reuseCredentials"):
        raise ValueError("Enter each S3 setting on a single line.")
    s = value.copy()
    if not re.fullmatch(r"[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]", s["bucket"]) or ".." in s["bucket"]:
        raise ValueError("Enter the existing bucket name using 3 to 63 lowercase letters, digits, dots or hyphens.")
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,62}", s["region"]):
        raise ValueError("Enter the bucket region, such as eu-west-3 for AWS or auto for R2.")
    if s["endpoint"]:
        try:
            url = urlsplit(s["endpoint"])
            if (url.scheme not in ("https", "http") or not url.hostname or url.username is not None
                    or url.password is not None or url.query or url.fragment or url.port == 0
                    or any(ch.isspace() for ch in s["endpoint"])):
                raise ValueError
        except ValueError:
            raise ValueError("Enter an HTTP or HTTPS endpoint without credentials, query parameters or a fragment. Leave it blank for AWS.") from None
    if not reuse and (not s["accessKey"].strip() or not s["secretKey"].strip()):
        raise ValueError("Enter the access key and secret key for your S3 service.")
    s["prefix"] = s["prefix"].strip("/")
    if ".." in s["prefix"].split("/"):
        raise ValueError("Enter an S3 prefix without a “..” path component.")
    return s


def _settings(values, *, remote=False):
    if not isinstance(values, dict):
        raise ValueError("Some settings are missing. Reopen the installer and try again.")
    s = {"musicDirectory": "", "storage": {"kind": "local"}, **values}
    fields = {"directory", "musicDirectory", "port", "peerPort", "p2p", "domain", "access", "storage"}
    if s.keys() != fields:
        raise ValueError("Some settings are missing. Reopen the installer and try again.")
    for field in ("port", "peerPort"):
        if type(s[field]) is not int or not 1 <= s[field] <= 65535:
            raise ValueError("Choose a port between 1 and 65535.")
    if s["access"] not in ("local", "lan") or type(s["p2p"]) is not bool:
        raise ValueError("Choose one of the access and sharing options offered by the installer.")
    domain = s["domain"]
    label = r"[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?"
    if not isinstance(domain, str) or len(domain) > 253 or not re.fullmatch(rf"{label}(?:\.{label})*", domain):
        raise ValueError("Enter a host name or IPv4 address, such as localhost or music.home.")
    raw = s["directory"]
    if not isinstance(raw, str) or not raw.strip() or any(ord(ch) < 32 for ch in raw):
        raise ValueError("Enter a full folder path.")
    path = PurePosixPath(raw) if remote else Path(raw).expanduser()
    if (not path.is_absolute() or (not remote and path.is_symlink())
            or (remote and (path.anchor != "/" or ".." in path.parts))):
        raise ValueError("Choose a dedicated installation folder and use its absolute path.")
    path = path if remote else path.resolve()
    if path == type(path)(path.anchor):
        raise ValueError("Choose an installation folder below the root of the drive.")
    s["directory"] = str(path)
    s["storage"] = _storage(s["storage"])
    music = s["musicDirectory"]
    if not isinstance(music, str) or any(ord(ch) < 32 for ch in music):
        raise ValueError("Enter the full path to your music folder.")
    if music:
        if s["storage"]["kind"] == "s3":
            raise ValueError("Choose either a music folder on disk or an S3 bucket for this installation.")
        collection = PurePosixPath(music) if remote else Path(music).expanduser()
        if not collection.is_absolute() or (remote and (collection.anchor != "/" or ".." in collection.parts)):
            raise ValueError("Enter the full path to your music folder.")
        collection = collection if remote else collection.resolve()
        if (collection == type(collection)(collection.anchor) or path.is_relative_to(collection)
                or collection.is_relative_to(path)):
            raise ValueError("Choose a music folder separate from the installation folder.")
        if not remote and (not collection.is_dir() or not os.access(collection, os.R_OK | os.X_OK)):
            raise ValueError("The music folder must exist and be readable. Check its path or reconnect the drive.")
        s["musicDirectory"] = str(collection)
    return s


def _digest(data):
    return hashlib.sha256(data).hexdigest()


def _seal(data):
    """Detect accidental changes to files the wizard would otherwise reuse or replace."""
    return (HEADER + _digest(data) + "\n").encode() + data


def _unseal(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError("An installation file is missing or has been replaced. Check the selected folder.")
    head, _, data = path.read_bytes().partition(b"\n")
    if head != (HEADER + _digest(data)).encode():
        raise ValueError(f"{path.name} has changed. Keep this folder and manage it with Docker Compose, or choose an empty folder.")
    return data


def _state(directory):
    """Return persisted ownership and asset hashes. An unrelated checkout is never adopted."""
    if not directory.exists():
        return None
    if not directory.is_dir():
        raise ValueError("This path points to a file. Choose a folder.")
    env = directory / MANIFEST
    if not env.exists():
        if any(p.name not in (LOCK, ".DS_Store") for p in directory.iterdir()):
            raise ValueError("This folder already contains files. Choose an empty folder or an installation created with this installer.")
        return None
    raw = _unseal(env)
    try:
        first, variables = raw.decode().split("\n", 1)
        meta = json.loads(first.removeprefix("# "))
        project = meta["project"]
        if (meta["directory"] != str(directory) or not re.fullmatch(r"soundtime-[0-9a-f]{16}", project)
                or set(meta["assets"]) != ASSETS.keys()
                or not all(re.fullmatch("[0-9a-f]{64}", digest) for digest in meta["assets"].values())
                or not re.fullmatch(rf"COMPOSE_PROJECT_NAME={project}\nPOSTGRES_PASSWORD=[a-f0-9]{{64}}\nJWT_SECRET=[a-f0-9]{{64}}\n"
                                    r'(?:S3_ACCESS_KEY="[^\n]*"\nS3_SECRET_KEY="[^\n]*"\n)?', variables)):
            raise ValueError
    except (KeyError, TypeError, UnicodeError, ValueError, AttributeError):
        raise ValueError("The installation folder was moved or its credentials are incomplete. Restore the original configuration or choose an empty folder.") from None
    return meta


def _write(path, data):
    """Atomically replace a complete generated file. mkstemp grants owner-only access."""
    fd, name = tempfile.mkstemp(dir=path.parent, prefix=".st-write-")
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(name, path)
    finally:
        Path(name).unlink(missing_ok=True)


@contextlib.contextmanager
def _lock(directory):
    _state(directory)
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    path = directory / LOCK
    if path.is_symlink():
        raise ValueError("Choose a dedicated installation folder.")
    with path.open("a+b") as f:
        try:
            if os.name == "nt":
                import msvcrt
                f.seek(0)
                msvcrt.locking(f.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            raise RuntimeError("Another installation is using this folder. Wait for it to finish, then try again.") from None
        yield


def _prepare(directory):
    meta = _state(directory)
    if meta is None:
        meta = {"directory": str(directory), "project": "soundtime-" + secrets.token_hex(8),
                "assets": {name: _digest((ROOT / src).read_bytes()) for name, src in ASSETS.items()}}
        content = (f"# {json.dumps(meta, ensure_ascii=True)}\nCOMPOSE_PROJECT_NAME={meta['project']}\n"
                   f"POSTGRES_PASSWORD={secrets.token_hex(32)}\nJWT_SECRET={secrets.token_hex(32)}\n")
        _write(directory / MANIFEST, _seal(content.encode()))
    # Keep the version originally installed, including after a wizard upgrade or interrupted run.
    for name, src in ASSETS.items():
        dest = directory / name
        if dest.parent.is_symlink() or dest.is_symlink():
            raise ValueError("The Docker files must be inside the selected installation folder.")
        expected = meta["assets"][name]
        if dest.exists():
            if not dest.is_file() or _digest(dest.read_bytes()) != expected:
                raise ValueError(f"{name} has changed. Manage this installation with Docker Compose or select an empty folder.")
        else:
            data = (ROOT / src).read_bytes()
            if _digest(data) != expected:
                raise ValueError(f"Restore {name} from the original installation before continuing.")
            dest.parent.mkdir(exist_ok=True, mode=0o700)
            _write(dest, data)
    return meta


def _secret_variables(storage):
    if storage["kind"] != "s3":
        return ""
    def quoted(value):
        return '"' + value.replace("\\", "\\\\").replace('"', '\\"').replace("$", "$$") + '"'
    return (f"S3_ACCESS_KEY={quoted(storage['accessKey'])}\n"
            f"S3_SECRET_KEY={quoted(storage['secretKey'])}\n")


def _storage_environment(storage):
    """Compose values. Credentials stay in the private .env file."""
    if storage["kind"] != "s3":
        return {"STORAGE_BACKEND": "local"}
    env = {
        "STORAGE_BACKEND": "s3",
        "S3_REGION": storage["region"], "S3_BUCKET": storage["bucket"],
        "S3_PREFIX": storage["prefix"].replace("$", "$$"),
        "S3_ACCESS_KEY": "${S3_ACCESS_KEY:?}", "S3_SECRET_KEY": "${S3_SECRET_KEY:?}",
    }
    if storage["endpoint"]:
        env["S3_ENDPOINT"] = storage["endpoint"].replace("$", "$$")
    return env


def _override(settings, lan=False):
    host = "0.0.0.0" if lan else "127.0.0.1"
    p2p = settings["p2p"]
    peer = settings["peerPort"]
    ports = f"!override ['{peer}:{peer}/udp']" if p2p else "!reset []"
    collection = ""
    if settings.get("musicDirectory"):
        # A double-quoted YAML scalar accepts JSON string escaping. Compose
        # must separately escape literal dollar signs in host paths.
        source = json.dumps(settings["musicDirectory"].replace("$", "$$"), ensure_ascii=False)
        collection = f"""    volumes:
      - type: bind
        source: {source}
        target: /data/music/collection
        read_only: true
        bind:
          create_host_path: false
"""
    storage = settings.get("storage", {"kind": "local"})
    storage_yaml = ""
    if storage["kind"] == "s3":
        for key, value in _storage_environment(storage).items():
            storage_yaml += f"      {key}: {json.dumps(value, ensure_ascii=False)}\n"
        if not storage["endpoint"]:
            # The Rust SDK distinguishes an absent endpoint from Some("").
            storage_yaml += "      S3_ENDPOINT: !reset null\n"
    return f"""services:
  redis:
    container_name: !reset null
    ports: !reset []
  backend:
    ports: {ports}
{collection}    environment:
{storage_yaml}      SOUNDTIME_DOMAIN: "{settings['domain']}"
      SOUNDTIME_SCHEME: http
      P2P_ENABLED: "{str(p2p).lower()}"
      P2P_BIND_PORT: "{peer}"
      JWT_SECRET: ${{JWT_SECRET:?}}
      DATABASE_URL: postgres://soundtime:${{POSTGRES_PASSWORD:?}}@postgres:5432/soundtime
  postgres:
    environment:
      POSTGRES_USER: soundtime
      POSTGRES_DB: soundtime
      POSTGRES_PASSWORD: ${{POSTGRES_PASSWORD:?}}
  frontend:
    ports: !reset []
  nginx:
    ports: !override ['{host}:{settings['port']}:80']
""".encode()


def _saved_plan(directory, data):
    """Recover only public settings from an installer-owned Compose override."""
    try:
        yaml = data.decode("utf-8")
        listen = re.search(r"^    ports: !override \['(127\.0\.0\.1|0\.0\.0\.0):(\d+):80'\]$", yaml, re.M)
        if not listen:
            raise ValueError
        options = {key: json.loads(value) for key, value in re.findall(
            r'^      (SOUNDTIME_DOMAIN|P2P_ENABLED|P2P_BIND_PORT|STORAGE_BACKEND|S3_REGION|S3_BUCKET|S3_ENDPOINT|S3_PREFIX): (".*")$', yaml, re.M
        )}
        plan = dict(defaults(), directory=str(directory), port=int(listen[2]),
                    access="local" if listen[1] == "127.0.0.1" else "lan",
                    domain=options["SOUNDTIME_DOMAIN"], p2p=options["P2P_ENABLED"] == "true",
                    peerPort=int(options["P2P_BIND_PORT"]))
        source = re.search(r'^        source: (".*")$', yaml, re.M)
        if source:
            plan["musicDirectory"] = json.loads(source[1]).replace("$$", "$")
        if options.get("STORAGE_BACKEND") == "s3":
            plan["storage"] = {"kind": "s3", "reuseCredentials": True,
                               "region": options["S3_REGION"], "bucket": options["S3_BUCKET"],
                               "endpoint": options.get("S3_ENDPOINT", "").replace("$$", "$"),
                               "prefix": options["S3_PREFIX"].replace("$$", "$")}
        # Only trust a layout we know how to reproduce without losing a setting.
        if _override(plan, lan=plan["access"] == "lan") != data:
            raise ValueError
        return plan
    except (UnicodeError, KeyError, ValueError, AttributeError, TypeError):
        raise ValueError("These settings could not be recovered automatically. Restore the original configuration or choose an empty folder.") from None


def recommend(directory=None):
    """Suggest settings on this machine without creating files or returning credentials."""
    plan = defaults()
    path = Path(plan["directory"] if directory is None else directory).expanduser()
    if not path.is_absolute() or path.is_symlink() or path == Path(path.anchor):
        raise ValueError("Choose a dedicated installation folder and use its absolute path.")
    path = path.resolve()
    plan["directory"] = str(path)
    if (path / MANIFEST).is_file() and _state(path):
        override = path / "compose.override.yaml"
        if override.exists() or override.is_symlink():
            return _saved_plan(path, _unseal(override))
        if b"\nS3_ACCESS_KEY=" in _unseal(path / MANIFEST):
            raise ValueError("Restore compose.override.yaml from the original installation before continuing.")
    for port in range(8880, 8900):
        try:
            _check_port(port, "127.0.0.1", socket.SOCK_STREAM, None, "nginx", 80)
        except ValueError:
            continue
        plan["port"] = port
        break
    return plan


def _check_saved_credentials(path, storage):
    override = path / "compose.override.yaml"
    if (not override.is_file() or _saved_plan(path, _unseal(override))["storage"] != storage
            or b"\nS3_ACCESS_KEY=" not in _unseal(path / MANIFEST)):
        raise ValueError("To change the S3 storage settings, enter the bucket credentials again.")


def _configure(directory, settings, lan=False, *, keep_credentials=False):
    path = directory / "compose.override.yaml"
    data = _override(settings, lan)
    previous = _unseal(path) if path.exists() or path.is_symlink() else None
    storage = settings.get("storage", {"kind": "local"})
    if storage.get("reuseCredentials"):
        _check_saved_credentials(directory, storage)
    elif not keep_credentials:
        envpath = directory / MANIFEST
        env = _unseal(envpath)
        core = env.partition(b"\nS3_ACCESS_KEY=")[0].rstrip(b"\n") + b"\n"
        new_env = core + _secret_variables(storage).encode()
        if env != new_env:
            _write(envpath, _seal(new_env))
    if previous != data:
        _write(path, _seal(data))


def _docker_env():
    # The instance credentials and service options come from this installation.
    # Preserve the CLI's context, credentials, proxy, PATH and system environment.
    keys = set(re.findall(r"\$\{(\w+)", (ROOT / "docker-compose.yml").read_text()))
    keys.update(("SOUNDTIME_S3_ACCESS_KEY", "SOUNDTIME_S3_SECRET_KEY",
                 "SOUNDTIME_ADMIN_PASSWORD", "SOUNDTIME_BOOTSTRAP_PASSWORD"))
    return {k: v for k, v in os.environ.items() if k not in keys and not k.startswith("COMPOSE_")}


def _docker(*args, cwd=None, timeout=12):
    try:
        return subprocess.run(["docker", *args], cwd=cwd, env=_docker_env(), capture_output=True,
                              text=True, encoding="utf-8", errors="replace", timeout=timeout,
                              stdin=subprocess.DEVNULL, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    except subprocess.TimeoutExpired:
        raise RuntimeError("Docker took too long to respond. Check Docker and your connection, then try again.") from None


def _docker_ready():
    if shutil.which("docker") is None:
        raise ValueError("Install Docker Desktop on Mac or Windows, or Docker Engine with Compose on Linux, then run the checks again.")
    endpoint = os.environ.get("DOCKER_HOST")
    if not endpoint or os.environ.get("DOCKER_CONTEXT"):
        context = _docker("context", "inspect", "--format", "{{.Endpoints.docker.Host}}")
        endpoint = context.stdout.strip() if context.returncode == 0 else ""
    if not endpoint:
        raise ValueError("Open Docker Desktop or start Docker Engine, then run the checks again.")
    if not (endpoint.startswith("unix:///") or endpoint.lower().replace("\\", "/").startswith("npipe:////./pipe/")):
        raise ValueError("Select this computer’s Docker engine. To install on a remote host, use the remote installation option or run the installer there.")
    result = _docker("info", "--format", "{{.OSType}}/{{.Architecture}}")
    if result.returncode:
        raise ValueError("Docker is installed. Start it and make sure your account can access it, then try again.")
    if result.stdout.strip().lower() not in {"linux/x86_64", "linux/amd64", "linux/arm64", "linux/aarch64"}:
        raise ValueError("SoundTime runs in Linux x86-64 or ARM64 containers. On Windows, switch Docker Desktop to Linux containers.")
    version = _docker("compose", "version", "--short")
    match = re.match(r"v?(\d+)\.(\d+)\.(\d+)", version.stdout.strip())
    if version.returncode or not match or tuple(map(int, match.groups())) < MIN_COMPOSE:
        raise ValueError("Update Docker Compose to version 2.24.4 or later, then try again.")
    return f"Docker is running. Compose {match[0]} is available."


def _port_owner(project, service, internal_port, udp=False):
    # Only the container in this specific installer-owned project can occupy its port on retry.
    cmd = ["ps", "--filter", f"label=com.docker.compose.project={project}", "--filter",
           f"label=com.docker.compose.service={service}", "--format", "{{.ID}}"]
    res = _docker(*cmd)
    cid = res.stdout.strip()
    if res.returncode or not re.fullmatch(r"[a-f0-9]{12,64}", cid):
        return set()
    spec = f"{internal_port}/{'udp' if udp else 'tcp'}"
    result = _docker("port", cid, spec)
    if result.returncode:
        return set()
    return {int(m) for m in re.findall(r":(\d+)\s*$", result.stdout, re.MULTILINE)}


def _check_port(port, host, kind, meta, service, internal):
    try:
        with socket.socket(socket.AF_INET, kind) as sock:
            if os.name == "nt":
                sock.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
            sock.bind((host, port))
    except OSError:
        if meta and port in _port_owner(meta["project"], service, internal, kind == socket.SOCK_DGRAM):
            return
        raise ValueError(f"Port {port} is in use or reserved. Choose another port in the settings.") from None


def _check_error(exc):
    if not isinstance(exc, OSError):
        return str(exc)
    reasons = {
        errno.EACCES: "Permission denied. Check your account's access.",
        errno.EPERM: "Permission denied. Check your account's access.",
        errno.ENOENT: "The required file or command could not be found.",
        errno.ENOSPC: "The disk is full. Free space and try again.",
        errno.EROFS: "The disk is read-only. Choose a writable location.",
    }
    detail = reasons.get(exc.errno, "The operating system could not complete this check.")
    return f"{detail} ({exc.filename})" if exc.filename else detail


def preflight(settings):
    checks = []

    def check(key, label, action):
        try:
            message = action()
            checks.append(dict(id=key, label=label, ok=True, detail=message))
            return True
        except (ValueError, RuntimeError, OSError) as exc:
            checks.append(dict(id=key, label=label, ok=False, detail=_check_error(exc)))
            return False

    try:
        s = _settings(settings)
    except (ValueError, OSError, RuntimeError) as exc:
        return {"ok": False, "checks": [dict(id="settings", label="Settings", ok=False, detail=_check_error(exc))]}
    path = Path(s["directory"])
    meta = None

    def directory():
        nonlocal meta
        meta = _state(path)
        if s["storage"].get("reuseCredentials"):
            _check_saved_credentials(path, s["storage"])
        ancestor = path
        while not ancestor.exists():
            ancestor = ancestor.parent
        if not ancestor.is_dir() or not os.access(ancestor, os.W_OK):
            raise ValueError("Choose a folder your account can write to.")
        return "The existing installation will resume." if meta else "The folder is available for SoundTime."

    def ports():
        _check_port(s["port"], "0.0.0.0" if s["access"] == "lan" else "127.0.0.1",
                    socket.SOCK_STREAM, meta, "nginx", 80)
        if s["p2p"]:
            _check_port(s["peerPort"], "0.0.0.0", socket.SOCK_DGRAM, meta, "backend", s["peerPort"])
        return "The selected ports are available."

    docker_ok = check("docker", "Docker and Compose", _docker_ready)
    check("directory", "Installation folder", directory)
    if not docker_ok:
        meta = None
    check("ports", "Network ports", ports)
    if s["storage"]["kind"] == "s3":
        checks.append(dict(id="storage", label="S3 configuration", ok=True,
                           detail="The S3 settings are filled in. Upload and play a track to verify access to the bucket."))
    return {"ok": all(c["ok"] for c in checks), "checks": checks}


def _compose(directory, args, timeout, on_error):
    result = _docker("compose", "--env-file", ".env", "-f", "compose.yaml", "-f", "compose.override.yaml",
                     *args, cwd=directory, timeout=timeout)
    if result.returncode:
        detail = result.stderr.lower()
        if any(s in detail for s in ("port is already allocated", "address already in use", "ports are not available")):
            raise RuntimeError("Another application just reserved the port. Change the port in settings and restart the installation.")
        if "no space left" in detail or "disk quota" in detail:
            raise RuntimeError("Docker’s disk is full. Free some space and try again using the same folder.")
        raise RuntimeError(on_error)
    return result.stdout


def _download(directory, report):
    images = _compose(directory, ["config", "--images"], 30, "The Docker configuration is invalid. Check your Compose version.")
    refs = list(dict.fromkeys(images.splitlines()))
    if not refs:
        raise RuntimeError("Docker Compose found no images in the configuration.")
    for i, image in enumerate(refs, 1):
        # Compose pulls :latest even with --pull missing. Inspect first so that
        # retries can use a complete local cache, including the SoundTime images.
        cached = _docker("image", "inspect", "--format", "{{.Id}}", image)
        if cached.returncode == 0:
            report("download", f"{i}/{len(refs)} · {image} is already available on this computer.")
            continue
        report("download", f"{i}/{len(refs)} · Downloading {image}.")
        if _docker("pull", "--quiet", image, timeout=1800).returncode:
            raise RuntimeError(f"Docker could not download {image}. Check your connection, disk space and registry access, then try again.")


def _get(port, path):
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
    try:
        conn.request("GET", path)
        response = conn.getresponse()
        if response.status != 200:
            raise ValueError
        if path == "/setup":
            if "text/html" not in response.getheader("Content-Type", "") or not response.read(1024).strip():
                raise ValueError
            return None
        return json.loads(response.read(16384))
    finally:
        conn.close()


def _readiness(port, wait=120):
    deadline = time.monotonic() + wait
    while True:
        try:
            if _get(port, "/healthz")["status"] == "ok":
                status = _get(port, "/api/setup/status")
                if type(status["setup_complete"]) is bool and type(status["has_admin"]) is bool:
                    _get(port, "/setup")
                    return status
        except (OSError, ValueError, KeyError, TypeError, http.client.HTTPException):
            pass
        if time.monotonic() >= deadline:
            raise RuntimeError("SoundTime is still starting or a service needs attention. Retry with the same folder. Run “docker compose logs --tail=100” in that folder to see which service is affected.")
        time.sleep(1)


def _lan_host(settings):
    if settings["domain"] != "localhost":
        return settings["domain"]
    try:
        # UDP connect selects the local route without sending a packet.
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.connect(("192.0.2.1", 80))
            return sock.getsockname()[0]
    except OSError:
        return platform.node() or "localhost"


def _result(settings, status, lan=False):
    base = f"http://127.0.0.1:{settings['port']}"
    result = {"directory": settings["directory"], "url": base + ("/" if status["setup_complete"] else "/setup"),
              "lanPending": settings["access"] == "lan" and not lan}
    if settings.get("musicDirectory"):
        result["musicDirectory"] = settings["musicDirectory"]
    if settings.get("storage", {}).get("kind") == "s3":
        result["storage"] = {key: settings["storage"][key] for key in ("kind", "region", "bucket", "prefix")}
    if lan:
        result["lanUrl"] = f"http://{_lan_host(settings)}:{settings['port']}/"
    return result


def _start(directory, *services):
    _compose(directory, ["up", "-d", "--pull", "never", *services], 600, "Docker could not start the services. Check disk space and Docker Desktop file sharing, then try again. Run “docker compose logs --tail=100” in this folder for details.")


def _publish(directory, settings, report):
    status = _readiness(settings["port"], wait=3)
    if not (status["has_admin"] and status["setup_complete"]):
        raise RuntimeError("Finish creating your account and setting up SoundTime, then enable network access again.")
    report("start", "Enabling access from your network.")
    _configure(directory, settings, lan=True, keep_credentials=True)
    try:
        _start(directory, "--no-deps", "nginx")
        report("verify", "Checking access to SoundTime.")
        status = _readiness(settings["port"])
    except (OSError, RuntimeError):
        # Recreate the private listener after a LAN bind error so activation
        # remains retryable from the same already-onboarded installation.
        _configure(directory, settings, keep_credentials=True)
        _start(directory, "--no-deps", "nginx")
        raise
    return _result(settings, status, lan=True)


def _validate_preflight(settings):
    outcome = preflight(settings)
    for check in outcome["checks"]:
        if not check["ok"]:
            raise ValueError(check["detail"])


def install(settings, report):
    s = _settings(settings)
    path = Path(s["directory"])
    report("prepare", "Checking Docker, the folder and the ports.")
    _validate_preflight(s)
    with _lock(path):
        _prepare(path)
        # Always bootstrap privately, even when the eventual destination is a LAN server.
        _configure(path, s)
        report("download", "Downloading missing images. Docker will reuse images already available.")
        _download(path, report)
        report("start", "Starting the music library and its services.")
        _start(path)
        report("verify", "Checking the server, database and setup page.")
        status = _readiness(s["port"])
        if s["access"] == "lan" and status["has_admin"] and status["setup_complete"]:
            return _publish(path, s, report)
        return _result(s, status)


def enable_lan(settings, report):
    s = _settings(settings)
    path = Path(s["directory"])
    if s["access"] != "lan" or not _state(path):
        raise ValueError("Select an existing installation configured for local network access.")
    _docker_ready()
    with _lock(path):
        state = _prepare(path)
        # A different process or instance must not be able to satisfy the bootstrap probe.
        if (s["port"] not in _port_owner(state["project"], "nginx", 80)
                or _unseal(path / "compose.override.yaml") not in (_override(s), _override(s, True))):
            raise ValueError("The installation or its settings have changed. Run the checks again using the same folder.")
        return _publish(path, s, report)


def export_project(settings, target, *, language="en"):
    if __package__:
        from . import project_export
    else:
        import project_export
    s = _settings(settings, remote=True)
    if s["storage"].get("reuseCredentials"):
        raise ValueError("Enter the bucket credentials to export an S3 configuration.")
    return project_export.archive(s, target, ROOT, _storage_environment(s["storage"]),
                                  _secret_variables(s["storage"]), language=language)
