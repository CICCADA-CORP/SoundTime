"""End-to-end check against published images. Requires a running local Docker daemon."""
from http.client import HTTPConnection
import json
import re
import os
import secrets
import socket
import subprocess
import sys
import tempfile
import time
import wave
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
ENTRY = Path(__file__).resolve().parents[1] / "launcher.py"


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
    def cli(*args, secret=None, success=True):
        env = {**os.environ, "SOUNDTIME_LANG": "fr", "LANGUAGE": "fr"}
        if secret:
            env["SOUNDTIME_ADMIN_PASSWORD"] = secret
        command = [os.environ["SOUNDTIME_TEST_CLI"]] if "SOUNDTIME_TEST_CLI" in os.environ else [sys.executable, str(ENTRY)]
        destination = []
        if "SOUNDTIME_TEST_SSH_PORT" in os.environ:
            destination = ["--target", "auto", "--ssh-host", "127.0.0.1", "--ssh-user", os.environ["SOUNDTIME_TEST_SSH_USER"],
                           "--ssh-port", os.environ["SOUNDTIME_TEST_SSH_PORT"]]
        result = subprocess.run([*command, "--json", *args, *destination],
                                stdin=subprocess.DEVNULL, capture_output=True, env=env, timeout=1000)
        if (result.returncode == 0) != success:
            raise AssertionError(f"CLI {args[0]} returned {result.returncode}: {result.stderr[-500:]!r}")
        return json.loads(result.stdout) if success else None

    with tempfile.TemporaryDirectory(prefix="soundtime-cli-test-") as temp:
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
            settings = ["--directory", str(path), "--port", str(web_port), "--music-directory", str(music)]
            cli("check", *settings, "--access", "lan")
            cli("install", *settings, "--access", "lan")
            cli("enable-lan", *settings, "--access", "lan", success=False)
            assert http(web_port, "/api/setup/status")["has_admin"] is False
            password = secrets.token_hex(24)
            onboarding = ["setup", "--port", str(web_port), "--admin-user", "installer_test",
                          "--admin-email", "installer@example.test", "--instance-name", "CLI test"]
            cli(*onboarding, secret=password)
            cli(*onboarding, secret=password)
            admin = http(web_port, "/api/auth/login", {"username": "installer_test", "password": password})
            admin_headers = {"Authorization": "Bearer " + admin["tokens"]["access_token"]}
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
            cli("install", *settings, "--access", "local")
            state = http(web_port, "/api/setup/status")
            assert state["has_admin"] and state["setup_complete"]
            assert http(web_port, "/api/tracks", headers=admin_headers)["data"][0]["id"] == catalog["data"][0]["id"]
            print("Verified fresh install, local onboarding, LAN guard, music import and retained catalog on retry.")
        finally:
            envfile = path / ".env"
            if envfile.is_file():
                project = re.search(r"^COMPOSE_PROJECT_NAME=(soundtime-[0-9a-f]{16})$", envfile.read_text(), re.M)
                if project and (path / "compose.override.yaml").exists():
                    subprocess.run(["docker", "compose", "-p", project[1], "-f", "compose.yaml", "-f",
                                    "compose.override.yaml", "down", "--volumes"], cwd=path, check=True)


if __name__ == "__main__":
    smoke()
