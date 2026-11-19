"""End-to-end check against published images. Requires a running local Docker daemon."""
from http.client import HTTPConnection
import json
import re
import secrets
import socket
import subprocess
import sys
import tempfile
import threading
import time
import wave
from pathlib import Path
from urllib.parse import urlsplit

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from installer.launcher import InstallerServer


def http(port, path, body=None, headers=None):
    conn = HTTPConnection("127.0.0.1", port, timeout=45)
    try:
        conn.request("GET" if body is None else "POST", path, None if body is None else json.dumps(body),
                     {"Content-Type": "application/json", **(headers or {})})
        res = conn.getresponse()
        assert 200 <= res.status < 300, (path, res.status)
        return json.load(res)
    finally:
        conn.close()


def smoke():
    with tempfile.TemporaryDirectory(prefix="soundtime-wizard-test-") as temp, InstallerServer(0) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        headers = {"Origin": f"http://127.0.0.1:{server.port}", "Authorization": "Bearer " + server.session}

        def api(endpoint, body=None):
            return http(server.port, "/api/" + endpoint, body, headers)

        def wait():
            last = ""
            deadline = time.monotonic() + 1920
            while time.monotonic() < deadline:
                state = api("status")
                if state["message"] != last:
                    print(state["message"], flush=True)
                    last = state["message"]
                if state["state"] != "running":
                    assert state["state"] == "complete", state["message"]
                    return state["result"]
                time.sleep(1)
            raise TimeoutError("Installation did not finish")

        path = Path(temp) / "Test Library"
        music = Path(temp) / "Albums ${HOME}"
        music.mkdir()
        track = music / "First Track.wav"
        with wave.open(str(track), "wb") as audio:
            audio.setparams((1, 2, 8000, 0, "NONE", "not compressed"))
            audio.writeframes(bytes(16000))
        try:
            with socket.socket() as sock:
                sock.bind(("127.0.0.1", 0))
                web_port = sock.getsockname()[1]
            settings = {**api("session")["defaults"], "directory": str(path), "port": web_port,
                        "access": "lan", "musicDirectory": str(music)}
            checks = api("check", settings)
            assert checks["ok"], checks
            api("install", settings)
            fresh = wait()
            assert fresh["lanPending"] and urlsplit(fresh["url"]).path == "/setup"
            api("lan", {})
            assert wait()["lanError"]
            assert http(web_port, "/api/setup/status")["has_admin"] is False
            admin = http(web_port, "/api/setup/admin", {
                "username": "installer_test", "email": "installer@example.test", "password": secrets.token_hex(24)
            })
            admin_headers = {"Authorization": "Bearer " + admin["tokens"]["access_token"]}
            http(web_port, "/api/setup/instance", {
                "instance_name": "Installer test", "instance_description": "Disposable installation"
            }, admin_headers)
            http(web_port, "/api/setup/complete", {
                "instance_private": True, "p2p_enabled": False, "open_registrations": False, "max_upload_size_mb": 50
            }, admin_headers)
            http(web_port, "/api/admin/storage/sync", {}, admin_headers)
            for _ in range(60):
                task = http(web_port, "/api/admin/storage/task-status", headers=admin_headers)
                if task["status"] == "completed":
                    break
                assert task["status"] != "error", "Collection synchronization failed"
                time.sleep(1)
            catalog = http(web_port, "/api/tracks", headers=admin_headers)
            assert len(catalog["data"]) == 1, "The selected collection did not appear in the catalog"
            # Retry using the same files and volumes. Keep the test project on loopback.
            settings["access"] = "local"
            api("install", settings)
            resumed = wait()
            assert urlsplit(resumed["url"]).path == "/" and not resumed["lanPending"]
            state = http(web_port, "/api/setup/status")
            assert state["has_admin"] and state["setup_complete"]
            assert http(web_port, "/api/tracks", headers=admin_headers)["data"][0]["id"] == catalog["data"][0]["id"]
            print("Verified fresh install, local onboarding, LAN guard, music import and retained catalog on retry.")
        finally:
            server.shutdown()
            thread.join()
            envfile = path / ".env"
            if envfile.is_file():
                project = re.search(r"^COMPOSE_PROJECT_NAME=(soundtime-[0-9a-f]{16})$", envfile.read_text(), re.M)
                if project and (path / "compose.override.yaml").exists():
                    subprocess.run(["docker", "compose", "-p", project[1], "-f", "compose.yaml", "-f",
                                    "compose.override.yaml", "down", "--volumes"], cwd=path, check=True)


if __name__ == "__main__":
    smoke()
