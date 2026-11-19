"""Deploy the bundled engine through the operator's existing trusted OpenSSH login."""
import base64
import http.client
import io
import ipaddress
import json
import re
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
TARGETS = ("nas", "server", "vps", "vm", "cloud")
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
    if not isinstance(user, str) or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_.-]{0,63}", user):
        raise ValueError("Indiquez le nom du compte Linux utilisé pour la connexion SSH.")
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
    return {"host": host, "user": user, "port": port}


def _client(ssh):
    client = shutil.which("ssh")
    if not client:
        raise ValueError("Installez le client OpenSSH sur cet ordinateur, ou choisissez le projet Docker à importer.")
    return [client, "-T", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes",
            "-o", "ConnectTimeout=12", "-o", "ConnectionAttempts=1",
            "-o", "ControlPath=none", "-o", "ControlMaster=no", "-o", "ControlPersist=no",
            "-o", "ForwardAgent=no", "-o", "ForwardX11=no", "-o", "PermitLocalCommand=no",
            "-o", "ServerAliveInterval=20", "-o", "ServerAliveCountMax=3",
            "-p", str(ssh["port"]), "-l", ssh["user"]]


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
    if b"python3" in diagnostic and (b"not found" in diagnostic or b"introuvable" in diagnostic):
        return "Installez Python 3.9 ou ultérieur sur le serveur ou utilisez le projet Docker à importer."
    return "Connexion SSH interrompue. Vérifiez l’adresse, le port, la clé et l’accès réseau au serveur."


def _call(ssh, action, settings, report):
    # Keep the agent archive and user options on separate lines. The bootstrap
    # unpacks the first line and the agent consumes the second.
    payload = base64.b64encode(_bundle()) + b"\n" + json.dumps({
        "action": action, "settings": settings,
    }).encode() + b"\n"
    cmd = _client(ssh) + ["-o", "ClearAllForwardings=yes", ssh["host"],
                          "python3 -I -c " + shlex.quote(BOOTSTRAP)]
    with tempfile.TemporaryFile() as errors, subprocess.Popen(
        cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors,
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
                                       stderr=subprocess.DEVNULL,
                                       creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.forward = address

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
        reconnect = (f"ssh -N -L 127.0.0.1:{local_port}:127.0.0.1:{port} "
                     f"-p {ssh['port']} -l {ssh['user']} {ssh['host']}")
        result.update(ssh=True, sshHost=ssh["host"], setupTunnel=reconnect, setupManualUrl=url)
        if ready:
            result["url"] = url
        else:
            self.close()
            result.update(url="", tunnelError="SoundTime fonctionne sur le serveur. Vérifiez le client SSH et la redirection TCP, puis ouvrez le tunnel indiqué pour accéder à votre bibliothèque.")
        return result
