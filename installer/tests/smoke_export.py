"""Run the NAS export against a real local S3-compatible bucket in disposable Docker volumes."""
import base64
import io
import json
import re
import secrets
import socket
import subprocess
import sys
import tempfile
import time
import wave
import zipfile
from http.client import HTTPConnection
from pathlib import Path
from xml.etree import ElementTree

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from installer import engine

S3_IMAGE = "chrislusf/seaweedfs@sha256:4b42c212543a0acd4c0e675215ecdcaf62a186ef98def76c7e12701871222d15"


def request(port, path, headers=None, body=None):
    conn = HTTPConnection("127.0.0.1", port, timeout=12)
    try:
        conn.request("GET" if body is None else "POST", path, body=body, headers=headers or {})
        response = conn.getresponse()
        return response.status, dict(response.getheaders()), response.read()
    finally:
        conn.close()


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def smoke():
    with tempfile.TemporaryDirectory(prefix="soundtime-nas-test-") as tmp:
        root = Path(tmp).resolve()
        web_port, s3_port = free_port(), free_port()
        storage = {"kind": "s3", "accessKey": "soundtimetest", "secretKey": secrets.token_hex(24),
                   "endpoint": "http://object-store:8333", "region": "us-east-1", "bucket": "test-music",
                   "prefix": "collection/audio"}
        settings = {**engine.defaults(), "directory": str(root), "port": web_port, "access": "lan", "storage": storage}
        name, archive = engine.export_project(settings, "nas")
        with zipfile.ZipFile(io.BytesIO(archive)) as project:
            project.extractall(root)
        model = json.loads((root / "compose.yaml").read_bytes())
        assert model["services"]["nginx"]["ports"][0]["host_ip"] == "0.0.0.0"
        # Bind every test service to loopback. The delivered NAS definition is otherwise unchanged.
        override = {"services": {"object-store": {
            "image": S3_IMAGE, "environment": {"AWS_ACCESS_KEY_ID": storage["accessKey"],
            "AWS_SECRET_ACCESS_KEY": storage["secretKey"], "S3_BUCKET": storage["bucket"]},
            "ports": [f"127.0.0.1:{s3_port}:8333"]}}}
        (root / "fixture.json").write_text(json.dumps(override))
        (root / "loopback.yaml").write_text(f"services:\n  nginx:\n    ports: !override ['127.0.0.1:{web_port}:80']\n")
        compose = ["docker", "compose", "-p", model["name"], "-f", "compose.yaml", "-f", "fixture.json", "-f", "loopback.yaml"]
        try:
            subprocess.run([*compose, "up", "-d"], cwd=root, check=True)
            for _ in range(120):
                try:
                    if request(web_port, "/setup")[0] == 401:
                        break
                except OSError:
                    pass
                time.sleep(1)
            status, headers, _ = request(web_port, "/setup")
            assert status == 401 and "Set-Cookie" not in headers
            guide = (root / "README.txt").read_text()
            password = re.search(r"Temporary password\s*:\s*(\S+)", guide)[1]
            basic = base64.b64encode(("soundtime:" + password).encode()).decode()
            for _ in range(60):
                status, headers, _ = request(web_port, "/setup", {"Authorization": "Basic " + basic})
                if status == 200:
                    break
                time.sleep(1)
            assert status == 200, ("setup", status)
            cookie = headers["Set-Cookie"].split(";", 1)[0]
            auth = {"Cookie": cookie, "Content-Type": "application/json"}
            status, _, body = request(web_port, "/api/setup/admin", auth, json.dumps({
                "username": "tester", "password": secrets.token_hex(20), "email": "tester@example.test"}))
            assert status == 201, ("admin", status)
            auth["Authorization"] = "Bearer " + json.loads(body)["tokens"]["access_token"]
            for route, payload in [("instance", {"instance_name": "Storage test", "instance_description": "QA"}),
                                   ("complete", {"p2p_enabled": False, "open_registrations": False, "max_upload_size_mb": 50})]:
                assert request(web_port, "/api/setup/" + route, auth, json.dumps(payload))[0] == 200
            audio = io.BytesIO()
            with wave.open(audio, "wb") as wav:
                wav.setparams((1, 2, 8000, 0, "NONE", "not compressed"))
                wav.writeframes(bytes(16000))
            auth["Content-Type"] = "multipart/form-data; boundary=soundtime-boundary"
            form = (b'--soundtime-boundary\r\nContent-Disposition: form-data; name="file"; filename="Test.wav"\r\n'
                    b'Content-Type: audio/wav\r\n\r\n' + audio.getvalue() + b'\r\n--soundtime-boundary--\r\n')
            status, _, body = request(web_port, "/api/upload", auth, form)
            assert 200 <= status < 300, ("S3 upload", status)
            # Query the bucket itself so a fallback to local storage cannot pass the test.
            credentials = f'user = "{storage["accessKey"]}:{storage["secretKey"]}"\n'
            listing = subprocess.run(["curl", "--config", "-", "--fail", "--silent", "--show-error",
                                      "--aws-sigv4", "aws:amz:us-east-1:s3",
                                      f"http://127.0.0.1:{s3_port}/test-music?list-type=2&prefix=collection%2Faudio%2F"],
                                     input=credentials, text=True, capture_output=True, check=True).stdout
            keys = [e.text for e in ElementTree.fromstring(listing).findall(".//{*}Key")]
            assert any(k.startswith("collection/audio/") for k in keys), "Upload missing from S3 prefix"
            active = root / "docker/access/bootstrap.active"
            active.rename(active.with_suffix(".done"))
            subprocess.run([*compose, "restart", "nginx"], cwd=root, check=True)
            for _ in range(40):
                try:
                    if request(web_port, "/api/setup/status")[0] == 200:
                        break
                except OSError:
                    pass
                time.sleep(.5)
            status, _, body = request(web_port, "/api/setup/status")
            assert status == 200 and json.loads(body)["setup_complete"]
            print("Verified NAS bootstrap challenge, session cookie, Bearer onboarding, removal of the bootstrap guard and upload into the chosen S3 prefix.", flush=True)
        finally:
            subprocess.run([*compose, "down", "--volumes"], cwd=root, check=True)


if __name__ == "__main__":
    smoke()
