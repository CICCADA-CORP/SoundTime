"""Prepare and run an isolated SoundTime Compose project on the local Docker engine."""

from __future__ import annotations

import contextlib
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
    fields = {"kind", "endpoint", "region", "bucket", "prefix", "accessKey", "secretKey"}
    if not isinstance(value, dict) or value.keys() != fields or value.get("kind") != "s3":
        raise ValueError("Choisissez le stockage sur disque ou renseignez les paramètres S3.")
    if any(not isinstance(v, str) or len(v) > 2048 or not all(ch.isprintable() for ch in v)
           for v in value.values()):
        raise ValueError("Les champs S3 doivent tenir chacun sur une ligne de texte.")
    s = value.copy()
    if not re.fullmatch(r"[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]", s["bucket"]) or ".." in s["bucket"]:
        raise ValueError("Indiquez le nom du bucket existant (3 à 63 lettres minuscules, chiffres, points ou tirets).")
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,62}", s["region"]):
        raise ValueError("Indiquez la région de votre bucket, par exemple eu-west-3 pour AWS ou auto pour R2.")
    if s["endpoint"]:
        try:
            url = urlsplit(s["endpoint"])
            if (url.scheme not in ("https", "http") or not url.hostname or url.username is not None
                    or url.password is not None or url.query or url.fragment or url.port == 0
                    or any(ch.isspace() for ch in s["endpoint"])):
                raise ValueError
        except ValueError:
            raise ValueError("L’adresse S3 doit être une URL HTTP ou HTTPS sans identifiants, paramètres ou fragment. Laissez-la vide pour AWS.") from None
    if not s["accessKey"].strip() or not s["secretKey"].strip():
        raise ValueError("Renseignez la clé d’accès et la clé secrète du service S3.")
    s["prefix"] = s["prefix"].strip("/")
    if ".." in s["prefix"].split("/"):
        raise ValueError("Indiquez un préfixe S3 sans composant « .. ».")
    return s


def _settings(values, *, remote=False):
    if not isinstance(values, dict):
        raise ValueError("Les réglages sont incomplets. Rechargez la page de l’assistant.")
    s = {"musicDirectory": "", "storage": {"kind": "local"}, **values}
    fields = {"directory", "musicDirectory", "port", "peerPort", "p2p", "domain", "access", "storage"}
    if s.keys() != fields:
        raise ValueError("Les réglages sont incomplets. Rechargez la page de l’assistant.")
    for field in ("port", "peerPort"):
        if type(s[field]) is not int or not 1 <= s[field] <= 65535:
            raise ValueError("Choisissez un numéro de port entre 1 et 65535.")
    if s["access"] not in ("local", "lan") or type(s["p2p"]) is not bool:
        raise ValueError("Choisissez une option d’accès et de partage proposée par l’assistant.")
    domain = s["domain"]
    label = r"[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?"
    if not isinstance(domain, str) or len(domain) > 253 or not re.fullmatch(rf"{label}(?:\.{label})*", domain):
        raise ValueError("Entrez un nom d’hôte ou une adresse IPv4, par exemple localhost ou musique.maison.")
    raw = s["directory"]
    if not isinstance(raw, str) or not raw.strip() or any(ord(ch) < 32 for ch in raw):
        raise ValueError("Choisissez un chemin de dossier complet.")
    path = PurePosixPath(raw) if remote else Path(raw).expanduser()
    if (not path.is_absolute() or (not remote and path.is_symlink())
            or (remote and (path.anchor != "/" or ".." in path.parts))):
        raise ValueError("Choisissez un dossier d’installation dédié avec un chemin absolu.")
    path = path if remote else path.resolve()
    if path == type(path)(path.anchor):
        raise ValueError("Choisissez un dossier d’installation dédié, sous la racine du disque.")
    s["directory"] = str(path)
    s["storage"] = _storage(s["storage"])
    music = s["musicDirectory"]
    if not isinstance(music, str) or any(ord(ch) < 32 for ch in music):
        raise ValueError("Choisissez le chemin complet de votre dossier de musique.")
    if music:
        if s["storage"]["kind"] == "s3":
            raise ValueError("Choisissez le dossier de musique sur disque ou le bucket S3 pour cette installation.")
        collection = PurePosixPath(music) if remote else Path(music).expanduser()
        if not collection.is_absolute() or (remote and (collection.anchor != "/" or ".." in collection.parts)):
            raise ValueError("Choisissez le chemin complet de votre dossier de musique.")
        collection = collection if remote else collection.resolve()
        if (collection == type(collection)(collection.anchor) or path.is_relative_to(collection)
                or collection.is_relative_to(path)):
            raise ValueError("Choisissez un dossier de musique dédié, séparé du dossier d’installation.")
        if not remote and (not collection.is_dir() or not os.access(collection, os.R_OK | os.X_OK)):
            raise ValueError("Le dossier de musique doit exister et être lisible. Vérifiez son chemin ou reconnectez le disque.")
        s["musicDirectory"] = str(collection)
    return s


def _digest(data):
    return hashlib.sha256(data).hexdigest()


def _seal(data):
    """Detect accidental changes to files the wizard would otherwise reuse or replace."""
    return (HEADER + _digest(data) + "\n").encode() + data


def _unseal(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError("Un fichier de l’installation est absent ou remplacé. Vérifiez le dossier choisi.")
    head, _, data = path.read_bytes().partition(b"\n")
    if head != (HEADER + _digest(data)).encode():
        raise ValueError(f"Le fichier {path.name} a été modifié. Conservez ce dossier et utilisez Docker Compose pour le gérer, ou choisissez un dossier vide.")
    return data


def _state(directory):
    """Return persisted ownership and asset hashes. An unrelated checkout is never adopted."""
    if not directory.exists():
        return None
    if not directory.is_dir():
        raise ValueError("Ce chemin désigne un fichier. Choisissez un dossier.")
    env = directory / MANIFEST
    if not env.exists():
        if any(p.name not in (LOCK, ".DS_Store") for p in directory.iterdir()):
            raise ValueError("Ce dossier contient déjà des fichiers. Choisissez un dossier vide ou une installation créée avec cet assistant.")
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
        raise ValueError("Le dossier de cette installation a été déplacé ou ses identifiants sont incomplets. Restaurez la configuration d’origine ou choisissez un dossier vide.") from None
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
        raise ValueError("Choisissez un dossier d’installation dédié.")
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
            raise RuntimeError("Une autre installation utilise déjà ce dossier. Attendez sa fin, puis réessayez.") from None
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
            raise ValueError("Les fichiers Docker de l’installation doivent se trouver dans le dossier choisi.")
        expected = meta["assets"][name]
        if dest.exists():
            if not dest.is_file() or _digest(dest.read_bytes()) != expected:
                raise ValueError(f"Le fichier {name} a été modifié. Gérez cette installation depuis Docker Compose ou choisissez un dossier vide.")
        else:
            data = (ROOT / src).read_bytes()
            if _digest(data) != expected:
                raise ValueError(f"Restaurez le fichier {name} de l’installation d’origine avant de reprendre.")
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


def _configure(directory, settings, lan=False, *, keep_credentials=False):
    path = directory / "compose.override.yaml"
    data = _override(settings, lan)
    previous = _unseal(path) if path.exists() or path.is_symlink() else None
    if not keep_credentials:
        envpath = directory / MANIFEST
        env = _unseal(envpath)
        core = env.partition(b"\nS3_ACCESS_KEY=")[0].rstrip(b"\n") + b"\n"
        new_env = core + _secret_variables(settings.get("storage", {"kind": "local"})).encode()
        if env != new_env:
            _write(envpath, _seal(new_env))
    if previous != data:
        _write(path, _seal(data))


def _docker_env():
    # The instance credentials and service options come from this installation.
    # Preserve the CLI's context, credentials, proxy, PATH and system environment.
    keys = set(re.findall(r"\$\{(\w+)", (ROOT / "docker-compose.yml").read_text()))
    return {k: v for k, v in os.environ.items() if k not in keys and not k.startswith("COMPOSE_")}


def _docker(*args, cwd=None, timeout=12):
    try:
        return subprocess.run(["docker", *args], cwd=cwd, env=_docker_env(), capture_output=True,
                              text=True, encoding="utf-8", errors="replace", timeout=timeout,
                              stdin=subprocess.DEVNULL, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    except subprocess.TimeoutExpired:
        raise RuntimeError("Docker met trop de temps à répondre. Vérifiez son état et votre connexion, puis réessayez.") from None


def _docker_ready():
    if shutil.which("docker") is None:
        raise ValueError("Installez Docker Desktop sur Mac ou Windows, ou Docker Engine avec Compose sur Linux, puis relancez la vérification.")
    endpoint = os.environ.get("DOCKER_HOST")
    if not endpoint or os.environ.get("DOCKER_CONTEXT"):
        context = _docker("context", "inspect", "--format", "{{.Endpoints.docker.Host}}")
        endpoint = context.stdout.strip() if context.returncode == 0 else ""
    if not endpoint:
        raise ValueError("Ouvrez Docker Desktop ou démarrez Docker Engine, puis relancez la vérification.")
    if not (endpoint.startswith("unix:///") or endpoint.lower().replace("\\", "/").startswith("npipe:////./pipe/")):
        raise ValueError("Sélectionnez le moteur Docker de cet ordinateur. Pour un serveur distant, ouvrez l’assistant sur ce serveur.")
    result = _docker("info", "--format", "{{.OSType}}/{{.Architecture}}")
    if result.returncode:
        raise ValueError("Docker est installé. Démarrez-le et vérifiez que votre compte y a accès, puis réessayez.")
    if result.stdout.strip().lower() not in {"linux/x86_64", "linux/amd64", "linux/arm64", "linux/aarch64"}:
        raise ValueError("SoundTime utilise des conteneurs Linux x86-64 ou ARM64. Sur Windows, activez les conteneurs Linux dans Docker Desktop.")
    version = _docker("compose", "version", "--short")
    match = re.match(r"v?(\d+)\.(\d+)\.(\d+)", version.stdout.strip())
    if version.returncode or not match or tuple(map(int, match.groups())) < MIN_COMPOSE:
        raise ValueError("Mettez à jour Docker Compose vers la version 2.24.4 ou ultérieure, puis réessayez.")
    return f"Docker est démarré. Compose {match[0]} est disponible."


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
        raise ValueError(f"Le port {port} est occupé ou réservé. Choisissez un autre numéro dans les réglages.") from None


def preflight(settings):
    checks = []

    def check(key, label, action):
        try:
            message = action()
            checks.append(dict(id=key, label=label, ok=True, detail=message))
            return True
        except (ValueError, RuntimeError, OSError) as exc:
            checks.append(dict(id=key, label=label, ok=False, detail=str(exc)))
            return False

    try:
        s = _settings(settings)
    except (ValueError, OSError, RuntimeError) as exc:
        return {"ok": False, "checks": [dict(id="settings", label="Réglages", ok=False, detail=str(exc))]}
    path = Path(s["directory"])
    meta = None

    def directory():
        nonlocal meta
        meta = _state(path)
        ancestor = path
        while not ancestor.exists():
            ancestor = ancestor.parent
        if not ancestor.is_dir() or not os.access(ancestor, os.W_OK):
            raise ValueError("Choisissez un dossier dans lequel votre compte peut écrire.")
        return "L’installation existante sera reprise." if meta else "Le dossier est disponible pour SoundTime."

    def ports():
        _check_port(s["port"], "0.0.0.0" if s["access"] == "lan" else "127.0.0.1",
                    socket.SOCK_STREAM, meta, "nginx", 80)
        if s["p2p"]:
            _check_port(s["peerPort"], "0.0.0.0", socket.SOCK_DGRAM, meta, "backend", s["peerPort"])
        return "Les ports choisis sont disponibles pour cette installation."

    docker_ok = check("docker", "Docker et Compose", _docker_ready)
    check("directory", "Dossier d’installation", directory)
    if not docker_ok:
        meta = None
    check("ports", "Ports réseau", ports)
    if s["storage"]["kind"] == "s3":
        checks.append(dict(id="storage", label="Configuration S3", ok=True,
                           detail="Paramètres renseignés. Un premier téléversement et sa lecture permettront de vérifier les droits sur le bucket."))
    return {"ok": all(c["ok"] for c in checks), "checks": checks}


def _compose(directory, args, timeout, on_error):
    result = _docker("compose", "--env-file", ".env", "-f", "compose.yaml", "-f", "compose.override.yaml",
                     *args, cwd=directory, timeout=timeout)
    if result.returncode:
        detail = result.stderr.lower()
        if any(s in detail for s in ("port is already allocated", "address already in use", "ports are not available")):
            raise RuntimeError("Un port vient d’être réservé par une autre application. Changez le port dans les réglages et relancez l’installation.")
        if "no space left" in detail or "disk quota" in detail:
            raise RuntimeError("Le disque utilisé par Docker est plein. Libérez de la place, puis réessayez avec le même dossier.")
        raise RuntimeError(on_error)
    return result.stdout


def _download(directory, report):
    images = _compose(directory, ["config", "--images"], 30, "La configuration Docker est invalide. Vérifiez votre version de Compose.")
    refs = list(dict.fromkeys(images.splitlines()))
    if not refs:
        raise RuntimeError("Docker Compose n’a trouvé aucune image dans la configuration.")
    for i, image in enumerate(refs, 1):
        # Compose pulls :latest even with --pull missing. Inspect first so that
        # retries can use a complete local cache, including the SoundTime images.
        cached = _docker("image", "inspect", "--format", "{{.Id}}", image)
        if cached.returncode == 0:
            report("download", f"{i}/{len(refs)} · {image} est disponible sur cet ordinateur.")
            continue
        report("download", f"{i}/{len(refs)} · Téléchargement de {image}.")
        if _docker("pull", "--quiet", image, timeout=1800).returncode:
            raise RuntimeError(f"Docker n’a pas pu télécharger {image}. Vérifiez votre connexion, l’espace disque et l’accès au registre, puis réessayez.")


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
            raise RuntimeError("SoundTime démarre encore ou un service rencontre un problème. Réessayez avec le même dossier. Le journal « docker compose logs --tail=100 » dans ce dossier indique le service concerné.")
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
    _compose(directory, ["up", "-d", "--pull", "never", *services], 600, "Docker n’a pas pu démarrer les services. Vérifiez l’espace disque et le partage du dossier dans Docker Desktop, puis réessayez. Le journal « docker compose logs --tail=100 » est disponible dans ce dossier.")


def _publish(directory, settings, report):
    status = _readiness(settings["port"], wait=3)
    if not (status["has_admin"] and status["setup_complete"]):
        raise RuntimeError("Terminez la création du compte et la configuration dans SoundTime, puis activez à nouveau l’accès réseau.")
    report("start", "Activation de l’accès depuis votre réseau.")
    _configure(directory, settings, lan=True, keep_credentials=True)
    try:
        _start(directory, "--no-deps", "nginx")
        report("verify", "Vérification de l’accès à SoundTime.")
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
    report("prepare", "Vérification de Docker, du dossier et des ports.")
    _validate_preflight(s)
    with _lock(path):
        _prepare(path)
        # Always bootstrap privately, even when the eventual destination is a LAN server.
        _configure(path, s)
        report("download", "Téléchargement des images manquantes. Docker réutilise les images déjà disponibles.")
        _download(path, report)
        report("start", "Démarrage de la bibliothèque et de ses services.")
        _start(path)
        report("verify", "Vérification du serveur, de la base de données et de la page de configuration.")
        status = _readiness(s["port"])
        if s["access"] == "lan" and status["has_admin"] and status["setup_complete"]:
            return _publish(path, s, report)
        return _result(s, status)


def enable_lan(settings, report):
    s = _settings(settings)
    path = Path(s["directory"])
    if s["access"] != "lan" or not _state(path):
        raise ValueError("Choisissez une installation existante configurée pour le réseau local.")
    _docker_ready()
    with _lock(path):
        state = _prepare(path)
        # A different process or instance must not be able to satisfy the bootstrap probe.
        if (s["port"] not in _port_owner(state["project"], "nginx", 80)
                or _unseal(path / "compose.override.yaml") not in (_override(s), _override(s, True))):
            raise ValueError("L’installation ou ses réglages ont changé. Relancez la vérification avec le même dossier.")
        return _publish(path, s, report)


def export_project(settings, target):
    if __package__:
        from . import project_export
    else:
        import project_export
    s = _settings(settings, remote=True)
    return project_export.archive(s, target, ROOT, _storage_environment(s["storage"]),
                                  _secret_variables(s["storage"]))
