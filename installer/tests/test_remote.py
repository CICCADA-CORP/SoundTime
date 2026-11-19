import base64
import io
import json
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

from installer import engine, remote


class RemoteTests(unittest.TestCase):
    def test_destination_uses_only_hostname_port_and_linux_account(self):
        for name in ("studio", "music.example.test", "192.0.2.3", "2001:db8::2"):
            config = {"host": name, "user": "soundtime", "port": 2222}
            self.assertEqual(remote.validate_destination("vm", config), config)
        for invalid in ({"host": "-abc"}, {"host": "my host"}, {"user": "a b"}, {"port": "22"}):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                remote.validate_destination("cloud", {"host": "server", "user": "music", "port": 22, **invalid})

    def test_agent_runs_from_bundle_alone_without_local_project(self):
        bundle = remote._bundle()
        with zipfile.ZipFile(io.BytesIO(bundle)) as archive:
            self.assertIn("docker/nginx.conf", archive.namelist())
            self.assertNotIn("installer/remote.py", archive.namelist())
        with tempfile.TemporaryDirectory() as folder:
            settings = {**engine.defaults(), "directory": str(Path(folder) / "new"), "port": 0}
            payload = base64.b64encode(bundle) + b"\n" + json.dumps({"action": "check", "settings": settings}).encode() + b"\n"
            # Run the exact transport bootstrap in isolated Python. Set the target
            # platform for this validation-only request on macOS and Windows CI.
            result = subprocess.run([sys.executable, "-I", "-c",
                                     "import platform; platform.system=lambda:'Linux'\n" + remote.BOOTSTRAP],
                                    input=payload, stdout=subprocess.PIPE, cwd=folder, check=True)
            item = json.loads(result.stdout.removeprefix(remote.PREFIX))
            self.assertEqual(item["kind"], "result")
            self.assertFalse(item["result"]["ok"])
            self.assertEqual(item["result"]["checks"][0]["id"], "settings")
            self.assertFalse(Path(settings["directory"]).exists())

    def test_ssh_failure_points_to_account_or_host_trust_without_echoing_stderr(self):
        self.assertIn("empreinte", remote._ssh_error(b"host key verification failed"))
        self.assertIn("refusé", remote._ssh_error(b"permission denied (publickey)"))
        self.assertIn("Python", remote._ssh_error(b"sh: python3: not found"))

    def test_remote_check_returns_a_correctable_diagnostic(self):
        with patch.object(remote, "_call", side_effect=RuntimeError("Clé inconnue.")):
            result = remote.preflight({"host": "music.test"}, {})
        self.assertFalse(result["ok"])
        self.assertEqual(result["checks"][0]["id"], "ssh")

    def test_tunnel_failure_preserves_success_and_explains_how_to_reconnect(self):
        ssh = {"host": "test.invalid", "port": 22, "user": "music"}
        remote_result = {"url": "http://127.0.0.1:9880/setup", "directory": "/music", "lanPending": True}
        with patch.object(remote, "_call", return_value=remote_result), patch.object(remote, "_client", return_value=["ssh"]), \
                patch.object(remote.subprocess, "Popen") as proc, patch.object(remote, "_tunnel_ready", return_value=False):
            proc.return_value.poll.return_value = None
            result = remote.RemoteInstaller().run(ssh, "install", {"port": 9880}, lambda *_: None)
        self.assertEqual(result["url"], "")
        self.assertTrue(result["tunnelError"])
        self.assertTrue(result["setupManualUrl"].endswith("/setup"))
        self.assertIn(":127.0.0.1:9880", result["setupTunnel"])
        proc.return_value.terminate.assert_called_once()


if __name__ == "__main__":
    unittest.main()
