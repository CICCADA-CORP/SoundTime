"""Deploy the bundled engine through the operator's existing trusted OpenSSH login."""
import base64
import hashlib
import http.client
import io
import ipaddress
import json
import os
import re
import secrets
import shlex
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import zipfile
from pathlib import Path
from urllib.parse import urlsplit


ROOT = Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent.parent))
TARGETS = ("nas", "server", "vps", "vm", "cloud", "auto", "linux", "macos", "windows")
PREFIX = b"SOUNDTIME-INSTALLER "
# Only this fixed program is passed to the remote shell. Settings and credentials
# travel over the encrypted stdin stream and never become command-line arguments.
BOOTSTRAP = """import base64, io, json, os, runpy, sys, tempfile, zipfile
if sys.version_info < (3, 9):
    print('SOUNDTIME-INSTALLER ' + json.dumps({'kind':'error','message':'Python 3.9 ou plus récent est requis sur le serveur.'}), flush=True)
    sys.exit(1)
with tempfile.TemporaryDirectory(prefix='soundtime-agent-') as directory:
    with zipfile.ZipFile(io.BytesIO(base64.b64decode(sys.stdin.buffer.readline()))) as archive:
        archive.extractall(directory)
    agent = os.path.join(directory, 'installer')
    sys.path.insert(0, agent)
    runpy.run_path(os.path.join(agent, 'remote_agent.py'), run_name='__main__')
"""


def validate_destination(target, ssh):
    if target not in TARGETS or not isinstance(ssh, dict):
        raise ValueError("Choisissez une destination et un compte SSH.")
    user, host, port = (ssh.get(k) for k in ("user", "host", "port"))
    username = r"[A-Za-z0-9_][A-Za-z0-9_.@-]{0,126}"
    if target in ("auto", "windows"):
        # A domain account or a local Windows account may contain a space.
        username = r"(?:[\w.-]+\\)?[\w][\w.@ -]{0,126}"
    if not isinstance(user, str) or len(user) > 160 or not re.fullmatch(username, user) or user.endswith(" "):
        raise ValueError("Indiquez le nom du compte utilisé pour la connexion SSH.")
    if not isinstance(host, str) or not host or len(host) > 253:
        raise ValueError("Indiquez le nom d’hôte ou l’adresse IP de la machine.")
    if ":" in host:
        try:
            ipaddress.IPv6Address(host)
        except ValueError:
            raise ValueError("L’adresse IPv6 du serveur est invalide.") from None
        if "%" in host:
            raise ValueError("Utilisez une adresse IPv6 sans identifiant d’interface.")
    elif not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9.-]*", host):
        raise ValueError("Indiquez un nom d’hôte ou une adresse IP, sans protocole ni chemin.")
    if type(port) is not int or not 1 <= port <= 65535:
        raise ValueError("Indiquez un port SSH entre 1 et 65535.")
    result = {"host": host, "user": user, "port": port}
    if target in ("auto", "linux", "macos", "windows"):
        result["os"] = target
    return result


def _ssh_environment():
    # OpenSSH may be configured to SendEnv *. Only the explicit agent request
    # should receive the storage credentials selected for this installation.
    private = {"SOUNDTIME_S3_ACCESS_KEY", "SOUNDTIME_S3_SECRET_KEY", "SOUNDTIME_ADMIN_PASSWORD",
               "SOUNDTIME_BOOTSTRAP_PASSWORD", "S3_ACCESS_KEY", "S3_SECRET_KEY", "POSTGRES_PASSWORD", "JWT_SECRET"}
    return {key: value for key, value in os.environ.items() if key.upper() not in private}


def _client(ssh, *, interactive=False):
    client = shutil.which("ssh")
    if not client:
        raise ValueError("Installez le client OpenSSH sur cet ordinateur, ou choisissez le projet Docker à importer.")
    return [client, "-T", "-o", "BatchMode=no" if interactive else "BatchMode=yes",
            "-o", "StrictHostKeyChecking=ask" if interactive else "StrictHostKeyChecking=yes",
            "-o", "ConnectTimeout=12", "-o", "ConnectionAttempts=1",
            "-o", "ControlPath=none", "-o", "ControlMaster=no", "-o", "ControlPersist=no",
            "-o", "ForwardAgent=no", "-o", "ForwardX11=no", "-o", "PermitLocalCommand=no",
            "-o", "ServerAliveInterval=20", "-o", "ServerAliveCountMax=3",
            "-p", str(ssh["port"]), "-l", ssh["user"]]


def prepare_login(ssh):
    """Explicit terminal-only first contact. OpenSSH owns fingerprint and passphrase prompts."""
    if not sys.stdin.isatty() or not sys.stderr.isatty():
        raise ValueError("Ouvrez un terminal pour confirmer l’empreinte SSH du serveur.")
    cmd = _client(ssh, interactive=True) + ["-o", "ClearAllForwardings=yes",
        "-o", "AddKeysToAgent=15m", ssh["host"], "exit 0"]
    return subprocess.run(cmd, env=_ssh_environment()).returncode == 0


def _bundle():
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as archive:
        for module in sorted((ROOT / "installer").glob("*.py")):
            if module.name not in ("launcher.py", "remote.py", "__init__.py"):
                archive.write(module, "installer/" + module.name)
        for path in ("docker-compose.yml", "docker/nginx.conf"):
            archive.write(ROOT / path, path)
        if not {"installer/engine.py", "installer/remote_agent.py"}.issubset(archive.namelist()):
            raise RuntimeError("L’agent SSH est absent de ce paquet. Rouvrez une distribution complète de l’assistant.")
    return output.getvalue()


def _ssh_error(diagnostic):
    if b"host key verification failed" in diagnostic or b"host identification has changed" in diagnostic:
        return "La clé de cet hôte n’est pas reconnue. Vérifiez son empreinte auprès de votre hébergeur, puis connectez-vous une première fois avec SSH depuis cet ordinateur."
    if b"permission denied" in diagnostic:
        return "Le serveur a refusé le compte ou sa clé SSH. Vérifiez le nom du compte et chargez sa clé dans votre agent SSH."
    if b"glibc_" in diagnostic or b"exec format error" in diagnostic or b"bad cpu type" in diagnostic:
        return "Le moteur est incompatible avec ce système. Sur un NAS, utilisez le projet Docker à importer."
    if any(name in diagnostic for name in (b"python3", b"python.exe", b"py.exe")) and (
            b"not found" in diagnostic or b"introuvable" in diagnostic or b"not recognized" in diagnostic):
        return "Installez Python 3.9 ou ultérieur sur le serveur ou utilisez le projet Docker à importer."
    return "Connexion SSH interrompue. Vérifiez l’adresse, le port, la clé et l’accès réseau au serveur."


def _powershell(script):
    """Use PowerShell's Unicode argument format across cmd.exe and PowerShell login shells."""
    encoded = base64.b64encode(("$ErrorActionPreference='Stop';" + script).encode("utf-16le")).decode("ascii")
    return "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand " + encoded


def _command(ssh, script):
    return _client(ssh) + ["-o", "ClearAllForwardings=yes", ssh["host"], script]


def _exec(ssh, script, *, input_file=None, timeout=35):
    try:
        return subprocess.run(_command(ssh, script), stdin=input_file or subprocess.DEVNULL,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout, env=_ssh_environment(),
                              creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    except subprocess.TimeoutExpired:
        raise RuntimeError("Le serveur a mis trop de temps à répondre. Vérifiez la connexion SSH.") from None


def detect(ssh):
    """Identify only the single operator-selected, authenticated host."""
    script = """os=$(uname -s) || exit 1
arch=$(uname -m) || exit 1
if [ "$os" = Darwin ] && [ "$(sysctl -in sysctl.proc_translated 2>/dev/null)" = 1 ]; then arch=arm64; fi
nas=none
if [ -f /etc/synoinfo.conf ]; then nas=synology; elif [ -f /etc/config/uLinux.conf ]; then nas=qnap; fi
printf 'SOUNDTIME-PLATFORM %s %s %s\\n' "$os" "$arch" "$nas" """
    result = _exec(ssh, "sh -c " + shlex.quote(script))
    if result.returncode == 255:
        raise RuntimeError(_ssh_error(result.stderr.lower()))
    match = re.search(rb"^SOUNDTIME-PLATFORM (Linux|Darwin) (\S+) (synology|qnap|none)\s*$", result.stdout, re.M)
    if result.returncode == 0 and match:
        os_name = "linux" if match[1] == b"Linux" else "macos"
        architecture = match[2].decode("ascii", errors="replace").lower()
        nas = match[3]
    else:
        result = _exec(ssh, _powershell(
            "$arch=$env:PROCESSOR_ARCHITEW6432; if (!$arch) {$arch=$env:PROCESSOR_ARCHITECTURE};"
            "if ($env:OS -eq 'Windows_NT') {Write-Output ('SOUNDTIME-PLATFORM ' + $arch)}"))
        if result.returncode == 255:
            raise RuntimeError(_ssh_error(result.stderr.lower()))
        match = re.search(rb"^SOUNDTIME-PLATFORM ([A-Za-z0-9_]+)\s*$", result.stdout, re.M)
        if result.returncode or not match:
            raise ValueError("Impossible d’identifier le serveur. Utilisez Linux, macOS ou Windows avec OpenSSH.")
        os_name, architecture, nas = "windows", match[1].decode().lower(), b"none"
    choices = {"amd64": "x64", "x86_64": "x64", "arm64": "arm64", "aarch64": "arm64"}
    if architecture not in choices:
        raise ValueError("L’installateur prend en charge les processeurs 64 bits Intel/AMD et ARM.")
    expected = ssh.get("os", "linux")
    if expected not in ("auto", os_name):
        raise ValueError("Le système détecté ne correspond pas à la destination choisie.")
    found = {"os": os_name, "arch": choices[architecture]}
    if nas != b"none":
        found["nas"] = nas.decode("ascii")
    return found


def _native_binary(platform):
    frozen = getattr(sys, "frozen", False)
    root = Path(sys.executable).resolve().parent.parent if frozen else ROOT / "bin"
    filename = "soundtime.exe" if platform["os"] == "windows" else "soundtime"
    binary = root / (platform["os"] + "-" + platform["arch"]) / filename
    if binary.is_file():
        return binary
    if frozen:
        raise ValueError("Extrayez l’archive universelle complète pour utiliser le moteur de la machine distante.")
    return None


def _staging_commands(platform, identifier, digest):
    """All path components are generated locally. No target settings enter shell commands."""
    if platform == "windows":
        directory = "$d=[IO.Path]::Combine([IO.Path]::GetTempPath(),'" + identifier + "');"
        paths = directory + "$f=[IO.Path]::Combine($d,'soundtime.exe');"
        remove = "if ([IO.File]::Exists($f)){[IO.File]::Delete($f)};if ([IO.Directory]::Exists($d)){[IO.Directory]::Delete($d)};"
        # The per-user TEMP directory supplies inherited access permissions.
        upload = paths + "New-Item -ItemType Directory -Path $d -ErrorAction Stop | Out-Null;try {" + (
            "$fs=[IO.File]::Open($f,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None);"
            "try {[Console]::OpenStandardInput().CopyTo($fs)} finally {$fs.Dispose()};"
            "$sha=[Security.Cryptography.SHA256]::Create();$reader=[IO.File]::OpenRead($f);"
            "try {$actual=[BitConverter]::ToString($sha.ComputeHash($reader)).Replace('-','')}"
            "finally {$reader.Dispose();$sha.Dispose()};"
            "if ($actual -ne '" + digest + "'){throw 'Invalid transfer'}"
        ) + "} catch {" + remove + "throw}"
        execute = (paths + "try {$OutputEncoding=New-Object Text.UTF8Encoding($false);"
                   "[Console]::ReadLine() | & $f --stdio-agent;$status=$LASTEXITCODE} finally {"
                   + remove + "};exit $status")
        return tuple(_powershell(command) for command in (upload, execute, paths + remove))
    paths = 'd="$HOME/' + identifier + '"; f="$d/soundtime"; '
    checksum = 'sha256sum "$f"' if platform == "linux" else 'shasum -a 256 "$f"'
    upload = ("set -eu; umask 077; " + paths + 'mkdir "$d"; '
              + '''trap 'rm -f "$f"; rmdir "$d"' EXIT; cat > "$f"; '''
              + 'sum=$(' + checksum + '); [ "${sum%% *}" = "' + digest + '" ]; chmod 700 "$f"; trap - EXIT')
    remove = 'rm -f "$f"; if [ -d "$d" ]; then rmdir "$d"; fi'
    execute = paths + "trap '" + remove + "' EXIT; " + '"$f" --stdio-agent'
    return upload, execute, paths + remove


def _python_command(platform):
    if platform != "windows":
        return "python3 -I -c " + shlex.quote(BOOTSTRAP)
    # Save the embedded source as a script so Win32 command-line quoting cannot
    # change Python string literals. Only a source checkout uses this fallback.
    program = base64.b64encode(BOOTSTRAP.encode()).decode("ascii")
    return _powershell(
        "$f=[IO.Path]::GetTempFileName();try {"
        "[IO.File]::WriteAllBytes($f,[Convert]::FromBase64String('" + program + "'));"
        "$lines=@([Console]::ReadLine(),[Console]::ReadLine());"
        "if (Get-Command py.exe -ErrorAction SilentlyContinue) {$lines | & py.exe -3 -I $f}"
        "else {$lines | & python.exe -I $f}; $status=$LASTEXITCODE"
        "} finally {[IO.File]::Delete($f)};exit $status")


def _call(ssh, action, settings, report):
    platform = detect(ssh)
    request = json.dumps({"action": action, "settings": settings, "os": platform["os"]}).encode() + b"\n"
    binary = _native_binary(platform)
    if binary is None:
        payload = base64.b64encode(_bundle()) + b"\n" + request
        return _exchange(_command(ssh, _python_command(platform["os"])), payload, action, report)
    digest = hashlib.sha256()
    with binary.open("rb") as stream:
        for part in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(part)
        stream.seek(0)
        identifier = ".soundtime-installer-" + secrets.token_hex(16)
        upload, execute, cleanup = _staging_commands(platform["os"], identifier, digest.hexdigest())
        result = _exec(ssh, upload, input_file=stream, timeout=900)
    if result.returncode:
        if result.returncode == 255:
            raise RuntimeError(_ssh_error(result.stderr.lower()))
        raise RuntimeError("Impossible de préparer le moteur sur le serveur. Vérifiez l’espace disponible et les droits du compte.")
    try:
        return _exchange(_command(ssh, execute), request, action, report)
    finally:
        # The agent is disposable. An interrupted connection must never replace
        # a successful installation result with a misleading deployment failure.
        try:
            _exec(ssh, cleanup)
        except (OSError, RuntimeError):
            pass


def _exchange(cmd, payload, action, report):
    with tempfile.TemporaryFile() as errors, subprocess.Popen(
        cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors, env=_ssh_environment(),
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)
    ) as process:
        timeout = threading.Timer(100 if action == "check" else 3 * 3600, process.kill)
        timeout.daemon = True
        timeout.start()
        result = None
        message = None
        try:
            try:
                process.stdin.write(payload)
                process.stdin.close()
            except BrokenPipeError:
                pass
            for line in process.stdout:
                if not line.startswith(PREFIX):
                    continue
                event = json.loads(line[len(PREFIX):])
                if event.get("kind") == "event":
                    report(event["stage"], event["message"])
                elif event.get("kind") == "error":
                    message = event["message"]
                elif event.get("kind") == "result":
                    result = event["result"]
            code = process.wait()
        finally:
            timeout.cancel()
            if process.poll() is None:
                process.kill()
        if message:
            raise ValueError(message)
        if code or not isinstance(result, dict):
            errors.seek(0)
            raise RuntimeError(_ssh_error(errors.read(16384).lower()))
        return result


def recommend(ssh, directory=None):
    return _call(ssh, "recommend", {"directory": directory} if directory else {}, lambda *_: None)


def preflight(ssh, settings):
    try:
        result = _call(ssh, "check", settings, lambda *_: None)
    except (ValueError, OSError, RuntimeError) as exc:
        message = str(exc) if not isinstance(exc, OSError) else "Le client SSH n’a pas pu démarrer."
        return {"ok": False, "checks": [{"id": "ssh", "label": "Connexion au serveur", "ok": False, "detail": message}]}
    result["checks"].insert(0, {"id": "ssh", "label": "Connexion au serveur", "ok": True,
                                "detail": "La clé SSH est reconnue et l’agent fonctionne sur " + ssh["host"] + "."})
    return result


def _close(process):
    if process and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=4)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def _tunnel_ready(process, local_port):
    deadline = time.monotonic() + 18
    while process.poll() is None and time.monotonic() < deadline:
        conn = http.client.HTTPConnection("127.0.0.1", local_port, timeout=2)
        try:
            conn.request("GET", "/api/setup/status")
            reply = conn.getresponse()
            if reply.status == 200 and isinstance(json.loads(reply.read(16384)).get("has_admin"), bool):
                return True
        except (OSError, ValueError, AttributeError, http.client.HTTPException):
            pass
        finally:
            conn.close()
        time.sleep(.2)
    return False


class RemoteInstaller:
    def __init__(self):
        self.tunnel = None
        self.forward = None
        self.local_port = None

    def close(self):
        _close(self.tunnel)
        self.tunnel = self.forward = self.local_port = None

    def _connect_tunnel(self, ssh, port):
        address = (ssh["user"], ssh["host"], ssh["port"], port)
        if self.forward == address and self.tunnel and self.tunnel.poll() is None:
            return
        self.close()
        with socket.socket() as candidate:
            candidate.bind(("127.0.0.1", 0))
            self.local_port = candidate.getsockname()[1]
        binding = f"127.0.0.1:{self.local_port}:127.0.0.1:{port}"
        cmd = _client(ssh) + ["-N", "-n", "-o", "ExitOnForwardFailure=yes", "-L", binding, ssh["host"]]
        self.tunnel = subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                       stderr=subprocess.DEVNULL, env=_ssh_environment(),
                                       creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.forward = address

    def open_tunnel(self, ssh, port):
        if type(port) is not int or not 1 <= port <= 65535:
            raise ValueError("Choisissez un numéro de port entre 1 et 65535.")
        self._connect_tunnel(ssh, port)
        if not _tunnel_ready(self.tunnel, self.local_port):
            self.close()
            raise RuntimeError("Le tunnel SSH ne rejoint pas SoundTime. Vérifiez le port web et la redirection TCP du serveur.")
        return self.local_port

    def run(self, ssh, action, settings, report):
        report("prepare", "Connexion à " + ssh["host"] + " avec votre clé SSH.")
        result = _call(ssh, action, settings, report)
        port = settings["port"]
        try:
            self._connect_tunnel(ssh, port)
            ready = _tunnel_ready(self.tunnel, self.local_port)
        except (OSError, ValueError):
            # Installation has already succeeded on the target. Keep its result
            # if this computer cannot launch a second SSH client or bind a port.
            ready = False
        local_port = self.local_port or port
        url = f"http://127.0.0.1:{local_port}{urlsplit(result['url']).path}"
        login = ssh["user"]
        if " " in login or "\\" in login:
            login = '"' + login + '"'
        reconnect = (f"ssh -N -L 127.0.0.1:{local_port}:127.0.0.1:{port} "
                     f"-p {ssh['port']} -l {login} {ssh['host']}")
        result.update(ssh=True, sshHost=ssh["host"], setupTunnel=reconnect, setupManualUrl=url)
        if ready:
            result["url"] = url
        else:
            self.close()
            result.update(url="", tunnelError="SoundTime fonctionne sur le serveur. Vérifiez le client SSH et la redirection TCP, puis ouvrez le tunnel indiqué pour accéder à votre bibliothèque.")
        return result
