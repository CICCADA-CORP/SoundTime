"""Launch an unpacked release, open its authenticated dashboard, then close it."""

import io
import json
import os
import queue
import re
import runpy
import subprocess
import sys
import tarfile
import tempfile
import threading
import zipfile
from html.parser import HTMLParser
from pathlib import Path
from urllib.request import ProxyHandler, Request, build_opener

from build import MAC_APP, PROGRAM


class LinkedAssets(HTMLParser):
    def __init__(self):
        super().__init__()
        self.paths = set()

    def handle_starttag(self, tag, attrs):
        for name, value in attrs:
            if name in ("src", "href") and value and value.startswith("/ui/"):
                self.paths.add(value)


def verify(archive):
    with tempfile.TemporaryDirectory(prefix="soundtime-release-test-") as tmp:
        folder = Path(tmp)
        runtime_tmp = folder / "runtime"
        runtime_tmp.mkdir()
        agent_path = None
        if archive.name == "SoundTime-Installer-sources.zip":
            with zipfile.ZipFile(archive) as release:
                release.extractall(folder)
            launcher = [sys.executable, str(folder / "installer" / "launcher.py")]
            agent_path = folder / "installer" / "remote.py"
        elif sys.platform == "darwin":
            subprocess.run(["ditto", "-x", "-k", str(archive), str(folder)], check=True)
            app = folder / f"{MAC_APP}.app" / "Contents"
            launcher = [str(app / "MacOS" / MAC_APP)]
            agent_path = app / "Resources" / "installer" / "remote.py"
        elif sys.platform == "win32":
            with zipfile.ZipFile(archive) as release:
                release.extract(f"{PROGRAM}.exe", folder)
            launcher = [str(folder / f"{PROGRAM}.exe")]
        else:
            with tarfile.open(archive, "r:gz") as release:
                release.extract(PROGRAM, folder, filter="data")
            launcher = [str(folder / PROGRAM)]
        environment = dict(os.environ, TMPDIR=str(runtime_tmp), TMP=str(runtime_tmp), TEMP=str(runtime_tmp))
        process = subprocess.Popen([*launcher, "--no-browser"], cwd=tmp, env=environment, stdout=subprocess.PIPE)
        urls = queue.Queue()

        def read_url():
            for line in process.stdout:
                match = re.search(rb"(http://127\.0\.0\.1:\d+)/#session=([A-Za-z0-9_-]+)", line)
                if match:
                    urls.put(tuple(part.decode("ascii") for part in match.groups()))
                    return
            urls.put(None)

        threading.Thread(target=read_url, daemon=True).start()
        http = build_opener(ProxyHandler({}))
        try:
            session = urls.get(timeout=60)
            if not session:
                raise RuntimeError("The packaged launcher closed without printing its session URL.")
            if agent_path is None:
                # One-file builds on Linux and Windows unpack here during this session.
                agent_path, = runtime_tmp.glob("_MEI*/installer/remote.py")
            # Exercise SSH agent assembly from the *shipped* module and its data,
            # even on runners that have no configured SSH login.
            agent = runpy.run_path(str(agent_path))["_bundle"]()
            with zipfile.ZipFile(io.BytesIO(agent)) as remote:
                for source in ("installer/engine.py", "installer/remote_agent.py",
                               "docker-compose.yml", "docker/nginx.conf"):
                    if not remote.read(source):
                        raise ValueError(f"The SSH bundle has an empty {source}.")
            origin, token = session
            with http.open(origin, timeout=10) as page:
                assets = LinkedAssets()
                assets.feed(page.read().decode())
            if not assets.paths:
                raise ValueError("The packaged interface has no linked assets.")
            for path in assets.paths:
                with http.open(origin + path, timeout=10) as asset:
                    if not asset.read(1):
                        raise ValueError(f"Empty packaged asset: {path}")
            headers = {"Authorization": f"Bearer {token}", "Origin": origin,
                       "Content-Type": "application/json"}
            with http.open(Request(origin + "/api/session", headers=headers), timeout=10) as info:
                session_info = json.load(info)
                if session_info["status"]["state"] != "idle":
                    raise ValueError("The packaged assistant is not ready.")
            params = {"target": "nas", "settings": {
                **session_info["defaults"], "directory": "/volume1/docker/soundtime", "access": "lan"
            }}
            export = Request(origin + "/api/export", data=json.dumps(params).encode(), headers=headers)
            with http.open(export, timeout=20) as result:
                if result.headers.get_content_type() != "application/zip":
                    raise ValueError("The NAS export did not return a Compose project.")
                with zipfile.ZipFile(io.BytesIO(result.read())) as project:
                    for name in ("compose.yaml", ".env", "docker/nginx.conf", "LISEZ-MOI.txt",
                                 "qnap-compose.yaml", "qnap/backend.env", "qnap/postgres.env"):
                        if not project.read(name).strip():
                            raise ValueError(f"The exported project has an empty {name}.")
            with http.open(Request(origin + "/api/close", data=b"{}", headers=headers), timeout=10):
                pass
            if process.wait(timeout=15):
                raise RuntimeError("The packaged launcher failed during shutdown.")
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=15)
            process.stdout.close()
        print(f"Packaged installer verified: {archive.name}")


if __name__ == "__main__":
    verify(Path(sys.argv[1]).resolve())
