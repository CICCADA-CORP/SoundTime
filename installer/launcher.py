"""Local browser-based installer for SoundTime. Run with Python 3.9+."""

import argparse
import copy
import hmac
import json
import mimetypes
import os
import platform
import secrets
import string
import sys
import threading
import time
import webbrowser
from functools import partial
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit

if __package__:
    from . import engine, remote
else:
    import engine
    import remote


ROOT = Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent.parent))
UI = ROOT / "installer" / "ui"
STAGES = ("prepare", "download", "start", "verify")
IDLE_TIMEOUT = 30 * 60


class RequestError(Exception):
    def __init__(self, status, message):
        super().__init__(message)
        self.status = status


class Installation:
    def __init__(self):
        self.lock = threading.Lock()
        self.settings = None
        self.remote = remote.RemoteInstaller()
        self.ssh = None
        self.closing = False
        self.last_request = time.monotonic()
        self.status = {"state": "idle", "stage": "prepare", "message": "", "events": []}

    def touch(self):
        with self.lock:
            self.last_request = time.monotonic()

    def snapshot(self):
        with self.lock:
            return copy.deepcopy(self.status)

    def request_close(self):
        with self.lock:
            if self.status["state"] == "running":
                raise RequestError(409, "L’installation est en cours. Attendez la fin de cette étape.")
            self.closing = True

    def expired(self):
        with self.lock:
            if self.status["state"] == "running":
                return False
            if time.monotonic() - self.last_request < IDLE_TIMEOUT:
                return False
            self.closing = True
            return True

    def start(self, settings=None, *, activate_lan=False, ssh=None):
        with self.lock:
            if self.closing:
                raise RequestError(409, "L’assistant se ferme. Rouvrez-le pour continuer.")
            if self.status["state"] == "running":
                raise RequestError(409, "Une opération est déjà en cours.")
            previous = self.status.get("result")
            if activate_lan:
                if not previous or not previous.get("lanPending"):
                    raise RequestError(409, "Terminez d’abord l’installation avec l’accès réseau sélectionné.")
                operation = partial(self.remote.run, self.ssh, "lan") if self.ssh else engine.enable_lan
                settings = self.settings
            else:
                self.ssh = ssh
                operation = partial(self.remote.run, ssh, "install") if ssh else engine.install
                self.settings = settings
            self.status = {"state": "running", "stage": "prepare", "message": "", "events": []}
            # Take the first snapshot under the same lock as the transition.
            initial = copy.deepcopy(self.status)

        def report(stage, message):
            with self.lock:
                if stage not in STAGES:
                    return
                self.status.update(stage=stage, message=message)
                self.status["events"].append({"stage": stage, "message": message})
                self.status["events"] = self.status["events"][-80:]

        def run():
            try:
                result = operation(settings, report)
            except Exception as exc:
                message = friendly_error(exc)
                with self.lock:
                    if activate_lan:
                        self.status.update(state="complete", result={**previous, "lanError": message})
                    else:
                        self.status.update(state="failed")
                    self.status["message"] = message
                    self.last_request = time.monotonic()
            else:
                with self.lock:
                    self.status.update(state="complete", result=result)
                    self.last_request = time.monotonic()

        threading.Thread(target=run, name="soundtime-install", daemon=False).start()
        return initial


def friendly_error(exc):
    if isinstance(exc, (ValueError, RuntimeError)) and str(exc):
        return str(exc)
    if isinstance(exc, PermissionError):
        return "Accès refusé. Vérifiez vos droits sur le dossier choisi et sur Docker."
    if isinstance(exc, OSError):
        return "Une opération sur la machine a échoué. Vérifiez le dossier, l’espace disponible et Docker."
    return "Une erreur inattendue a interrompu l’opération. Relancez la vérification."


def browse_folders(raw):
    """List only immediate, visible subdirectories of the operator's chosen folder."""
    if not isinstance(raw, str) or any(ord(ch) < 32 for ch in raw):
        raise RequestError(400, "Entrez un chemin de dossier valide.")
    home = Path.home()
    try:
        path = Path(raw).expanduser() if raw else home
        if not path.is_absolute():
            raise RequestError(400, "Entrez le chemin complet du dossier.")
        path = path.resolve(strict=True)
        with os.scandir(path) as entries:
            folders = []
            for entry in entries:
                if entry.name.startswith("."):
                    continue
                try:
                    if entry.is_dir():
                        folders.append({"name": entry.name, "path": entry.path})
                except OSError:
                    continue
    except (OSError, RuntimeError):
        raise RequestError(400, "Impossible d’ouvrir ce dossier. Sélectionnez un dossier existant et accessible.") from None
    roots = [{"name": "Dossier personnel", "path": str(home)}]
    if os.name == "nt":
        drives = (os.listdrives() if hasattr(os, "listdrives") else
                  [f"{letter}:\\" for letter in string.ascii_uppercase if os.path.isdir(f"{letter}:\\")])
        roots.extend({"name": drive, "path": drive} for drive in drives)
    else:
        roots.append({"name": "Ordinateur", "path": "/"})
        mounts = Path("/Volumes" if sys.platform == "darwin" else "/media")
        if mounts.is_dir():
            roots.append({"name": "Disques externes", "path": str(mounts)})
    return {"path": str(path), "parent": str(path.parent) if path.parent != path else None,
            "folders": sorted(folders, key=lambda item: item["name"].casefold()), "roots": roots}


class InstallerServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, port):
        super().__init__(("127.0.0.1", port), Handler)
        self.session = secrets.token_urlsafe(32)
        self.job = Installation()
        self.port = self.server_address[1]

    def service_actions(self):
        if self.job.expired():
            # HTTPServer.shutdown must be called outside the serving thread.
            threading.Thread(target=self.shutdown, daemon=True).start()

    def server_close(self):
        if hasattr(self, "job"):
            self.job.remote.close()
        super().server_close()


class Handler(BaseHTTPRequestHandler):
    server_version = "SoundTime"
    sys_version = ""

    def setup(self):
        super().setup()
        self.connection.settimeout(15)

    def log_message(self, *args):
        pass

    def reply(self, status, data, mime, filename=None):
        self.send_response(status)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        if filename:
            self.send_header("Content-Disposition", f'attachment; filename="{filename}"')
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; form-action 'none'; base-uri 'none'")
        self.end_headers()
        self.wfile.write(data)

    def json(self, status, value):
        self.reply(status, json.dumps(value, ensure_ascii=False).encode("utf-8"),
                   "application/json; charset=utf-8")

    def handle_request(self):
        try:
            host = self.headers.get("Host", "")
            port = self.server.port
            if host not in (f"localhost:{port}", f"127.0.0.1:{port}"):
                raise RequestError(403, "Utilisez l’adresse locale fournie par le lanceur.")
            path = urlsplit(self.path).path
            if path.startswith("/api/"):
                self.api(path, host)
            elif self.command == "GET":
                self.asset(path)
            else:
                raise RequestError(404, "Cette page est introuvable.")
        except RequestError as exc:
            self.json(exc.status, {"error": str(exc)})
        except (BrokenPipeError, ConnectionResetError, TimeoutError):
            return
        except Exception as exc:
            self.json(500, {"error": friendly_error(exc)})

    do_GET = do_POST = handle_request

    def asset(self, path):
        if path == "/":
            file = UI / "index.html"
        elif path.startswith("/ui/"):
            file = (UI / path[4:]).resolve()
            if not file.is_relative_to(UI.resolve()):
                raise RequestError(404, "Fichier introuvable.")
        else:
            raise RequestError(404, "Page introuvable.")
        if not file.is_file():
            raise RequestError(404, "Fichier de l’assistant introuvable.")
        mime = mimetypes.guess_type(str(file))[0] or "application/octet-stream"
        if file.suffix in (".js", ".mjs"):
            mime = "text/javascript; charset=utf-8"
        self.reply(200, file.read_bytes(), mime)

    def json_body(self):
        if self.headers.get_content_type() != "application/json":
            raise RequestError(415, "Les réglages doivent être envoyés au format JSON.")
        try:
            n = int(self.headers.get("Content-Length", ""))
        except ValueError:
            raise RequestError(400, "La taille de la requête est manquante.") from None
        if not 0 < n <= 16384:
            raise RequestError(413, "La requête dépasse la taille autorisée.")
        try:
            settings = json.loads(self.rfile.read(n).decode("utf-8"))
        except (ValueError, RecursionError):
            raise RequestError(400, "Les réglages reçus sont illisibles.") from None
        if not isinstance(settings, dict):
            raise RequestError(400, "Les réglages doivent être un objet JSON.")
        return settings

    def api(self, path, host):
        authorization = self.headers.get("Authorization", "")
        if not hmac.compare_digest(authorization.encode(), f"Bearer {self.server.session}".encode()):
            raise RequestError(401, "Rouvrez le lien personnel fourni par le lanceur SoundTime.")
        job = self.server.job
        job.touch()
        if self.command == "GET":
            if path == "/api/session":
                system = platform.system()
                self.json(200, {"defaults": engine.defaults(),
                                "platform": "macOS" if system == "Darwin" else system,
                                "status": job.snapshot()})
            elif path == "/api/status":
                self.json(200, job.snapshot())
            else:
                raise RequestError(404, "Action introuvable.")
            return
        if self.headers.get("Origin") != f"http://{host}":
            raise RequestError(403, "Rouvrez l’assistant depuis son adresse locale.")
        settings = self.json_body()
        if path == "/api/export":
            target = settings.get("target")
            if target not in remote.TARGETS:
                raise RequestError(400, "Choisissez le type de machine qui hébergera SoundTime.")
            try:
                name, archive = engine.export_project(settings.get("settings"), target)
            except ValueError as exc:
                raise RequestError(400, str(exc)) from exc
            self.reply(200, archive, "application/zip", filename=name)
        elif path in ("/api/remote/check", "/api/remote/install"):
            try:
                ssh = remote.validate_destination(settings.get("target"), settings.get("ssh"))
            except ValueError as exc:
                raise RequestError(400, str(exc)) from exc
            plan = settings.get("settings")
            if not isinstance(plan, dict):
                raise RequestError(400, "Les réglages de la machine distante sont incomplets.")
            if path.endswith("/check"):
                self.json(200, remote.preflight(ssh, plan))
            else:
                self.json(HTTPStatus.ACCEPTED, job.start(plan, ssh=ssh))
        elif path == "/api/folders":
            self.json(200, browse_folders(settings.get("path", "")))
        elif path == "/api/check":
            try:
                result = engine.preflight(settings)
            except ValueError as exc:
                raise RequestError(400, str(exc)) from exc
            self.json(200, result)
        elif path == "/api/install":
            self.json(HTTPStatus.ACCEPTED, job.start(settings))
        elif path == "/api/lan":
            self.json(HTTPStatus.ACCEPTED, job.start(activate_lan=True))
        elif path == "/api/close":
            job.request_close()
            self.json(200, {"closed": True})
            threading.Thread(target=self.server.shutdown, daemon=True).start()
        else:
            raise RequestError(404, "Action introuvable.")


def main():
    parser = argparse.ArgumentParser(description="Installer SoundTime sur cet ordinateur.")
    parser.add_argument("--no-browser", action="store_true", help="afficher le lien sans ouvrir le navigateur")
    parser.add_argument("--port", type=int, default=0, help="port local de l’assistant (0 = automatique)")
    args = parser.parse_args()
    if not 0 <= args.port <= 65535:
        parser.error("Le port doit être compris entre 0 et 65535.")
    # Finder launches applications with a shorter PATH than a terminal.
    if sys.platform == "darwin":
        paths = [os.environ.get("PATH", ""), "/usr/local/bin", "/opt/homebrew/bin",
                 str(Path.home() / ".docker/bin"), "/Applications/Docker.app/Contents/Resources/bin"]
        os.environ["PATH"] = os.pathsep.join(paths)
    try:
        server = InstallerServer(args.port)
    except OSError:
        parser.exit(1, "Impossible d’ouvrir ce port local. Choisissez un autre port avec --port.\n")
    with server:
        url = f"http://127.0.0.1:{server.port}/#session={server.session}"
        if sys.stdout:
            print("SoundTime • Assistant d’installation", url, sep="\n", flush=True)
        if not args.no_browser:
            webbrowser.open(url, new=2)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
