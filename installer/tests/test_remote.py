import base64
import io
import json
import hashlib
import os
import uuid
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import MagicMock, patch

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

    def test_detects_native_architecture_and_optional_nas(self):
        ssh = remote.validate_destination("auto", {"host": "nas.example", "port": 22, "user": "listener"})
        for output, expected in [
                (b"SOUNDTIME-PLATFORM Linux aarch64 synology", {"os": "linux", "arch": "arm64", "nas": "synology"}),
                (b"SOUNDTIME-PLATFORM Linux x86_64 qnap", {"os": "linux", "arch": "x64", "nas": "qnap"}),
                (b"SOUNDTIME-PLATFORM Darwin arm64 none", {"os": "macos", "arch": "arm64"})]:
            with self.subTest(output=output), patch.object(remote, "_exec",
                    return_value=subprocess.CompletedProcess([], 0, output, b"")):
                self.assertEqual(remote.detect(ssh), expected)
        windows_login = remote.validate_destination("windows", {**ssh, "user": r"OFFICE\Music User"})
        with patch.object(remote, "_exec", side_effect=[subprocess.CompletedProcess([], 1, b"", b"no sh"),
                subprocess.CompletedProcess([], 0, b"SOUNDTIME-PLATFORM AMD64\r\n", b"")]) as execute:
            self.assertEqual(remote.detect(windows_login), {"os": "windows", "arch": "x64"})
            self.assertIn("powershell.exe", execute.call_args.args[1])
        with patch.object(remote, "_exec", return_value=subprocess.CompletedProcess([], 0,
                b"SOUNDTIME-PLATFORM Linux armv7l none", b"")), self.assertRaisesRegex(ValueError, "64 bits"):
            remote.detect(ssh)
        with patch.object(remote, "_exec", return_value=subprocess.CompletedProcess([], 0,
                b"SOUNDTIME-PLATFORM Linux x86_64 none", b"")), self.assertRaisesRegex(ValueError, "correspond"):
            remote.detect(windows_login)

    def test_untrusted_host_ends_detection_before_another_probe(self):
        with patch.object(remote, "_exec", return_value=subprocess.CompletedProcess([], 255, b"",
                b"Host key verification failed")) as execute, self.assertRaisesRegex(RuntimeError, "empreinte"):
            remote.detect({"os": "auto"})
        self.assertEqual(execute.call_count, 1)

    def test_distribution_uses_matching_sibling_engine_and_rejects_incomplete_packages(self):
        with tempfile.TemporaryDirectory() as temp:
            bins = Path(temp) / "some space" / "bin"
            local = bins / "macos-arm64" / "soundtime"
            windows = bins / "windows-x64" / "soundtime.exe"
            windows.parent.mkdir(parents=True)
            windows.touch()
            with patch.object(sys, "frozen", True, create=True), patch.object(sys, "executable", str(local)):
                self.assertEqual(remote._native_binary({"os": "windows", "arch": "x64"}), windows.resolve())
                with self.assertRaisesRegex(ValueError, "archive universelle"):
                    remote._native_binary({"os": "linux", "arch": "x64"})

    def test_native_transfer_is_verified_and_cleaned_after_execution_error(self):
        with tempfile.TemporaryDirectory() as temp:
            binary = Path(temp) / "binary"
            binary.write_bytes(b"sample test file")
            ssh = {"host": "remote.example", "port": 2244, "user": "music"}
            settings = {"storage": {"secretKey": "private-test-value"}}
            with patch.object(remote, "detect", return_value={"os": "linux", "arch": "x64"}), \
                    patch.object(remote, "_native_binary", return_value=binary), \
                    patch.object(remote, "_exec", return_value=subprocess.CompletedProcess([], 0, b"", b"")) as execute, \
                    patch.object(remote, "_exchange", side_effect=RuntimeError("Disconnected")) as exchange, \
                    self.assertRaisesRegex(RuntimeError, "Disconnected"):
                remote._call(ssh, "check", settings, None)
            self.assertEqual(execute.call_count, 2)
            upload, cleanup = (call.args[1] for call in execute.call_args_list)
            self.assertIn(hashlib.sha256(binary.read_bytes()).hexdigest(), upload)
            self.assertIn("rmdir", cleanup)
            command, payload, _, _ = exchange.call_args.args
            self.assertIn("--stdio-agent", command[-1])
            self.assertNotIn("private-test-value", " ".join(command))
            self.assertEqual(json.loads(payload)["settings"], settings)

    def test_recommendations_send_only_safe_configuration(self):
        from installer import remote_agent
        import contextlib
        plan = {**engine.defaults(), "jwt": "private-test-value",
                "storage": {"kind": "s3", "bucket": "songs", "reuseCredentials": True, "secretKey": "private-test-value"}}
        stdin = MagicMock()
        stdin.buffer = io.BytesIO(b'{"action":"recommend","settings":{"directory":"/srv/music"}}\n')
        out = io.StringIO()
        with patch.object(engine, "recommend", create=True, return_value=plan) as recommend, \
                patch.object(sys, "stdin", stdin), contextlib.redirect_stdout(out):
            self.assertEqual(remote_agent.main(), 0)
        recommend.assert_called_once_with("/srv/music")
        self.assertNotIn("private-test-value", out.getvalue())
        self.assertIn("reuseCredentials", out.getvalue())

    def test_ssh_does_not_inherit_installer_credentials(self):
        keys = ("SOUNDTIME_S3_ACCESS_KEY", "SOUNDTIME_S3_SECRET_KEY", "SOUNDTIME_ADMIN_PASSWORD",
                "SOUNDTIME_BOOTSTRAP_PASSWORD")
        with patch.dict(os.environ, {**dict.fromkeys(keys, "private-test-value"), "SSH_AUTH_SOCK": "agent-socket"}):
            env = remote._ssh_environment()
        self.assertEqual(env["SSH_AUTH_SOCK"], "agent-socket")
        self.assertFalse(set(keys).intersection(env))

    def test_interactive_key_enrollment_requires_terminal_and_explicit_invocation(self):
        ssh = {"host": "example.test", "user": "music", "port": 22}
        with patch.object(sys.stdin, "isatty", return_value=False), self.assertRaisesRegex(ValueError, "terminal"):
            remote.prepare_login(ssh)
        with patch.object(sys.stdin, "isatty", return_value=True), patch.object(sys.stderr, "isatty", return_value=True), \
                patch.object(remote.shutil, "which", return_value="ssh"), \
                patch.object(subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
            self.assertTrue(remote.prepare_login(ssh))
            args = run.call_args.args[0]
            self.assertIn("StrictHostKeyChecking=ask", args)
            self.assertIn("BatchMode=no", args)
            self.assertIn("BatchMode=yes", remote._client(ssh))
            self.assertIn("StrictHostKeyChecking=yes", remote._client(ssh))

    def test_bad_frontend_request_is_a_protocol_error_from_package_entry_point(self):
        from installer import remote_agent
        import contextlib
        stdin = MagicMock()
        stdin.buffer = io.BytesIO(b'{"action":"unexpected","settings":{}}\n')
        output = io.StringIO()
        with patch.object(sys, "stdin", stdin), contextlib.redirect_stdout(output):
            self.assertEqual(remote_agent.main(), 1)
        self.assertEqual(json.loads(output.getvalue()[len(remote_agent.PREFIX):])["kind"], "error")

    @unittest.skipUnless(os.name == "nt", "exercise Windows PowerShell on a real Windows runner")
    def test_windows_binary_stream_and_cleanup(self):
        with tempfile.TemporaryDirectory(prefix="SoundTime test ") as temp:
            root = Path(temp)
            source = root / "Reader.cs"
            source.write_text('using System; public class Reader { public static int Main(string[] a) { '
                              'Console.WriteLine(Console.ReadLine()); return a[0]=="--stdio-agent" ? 0 : 1; }}')
            exe = root / "Reader.exe"
            result = subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command",
                "Add-Type -Path $env:ST_CS -OutputAssembly $env:ST_EXE -OutputType ConsoleApplication"],
                env=dict(os.environ, ST_CS=str(source), ST_EXE=str(exe)), capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            data = exe.read_bytes()
            cmds = remote._staging_commands("windows", ".soundtime-test-" + uuid.uuid4().hex,
                                            hashlib.sha256(data).hexdigest())
            # The encoded argument itself contains no shell quoting or spaces.
            try:
                upload = subprocess.run(cmds[0].split(), input=data, capture_output=True)
                self.assertEqual(upload.returncode, 0, upload.stderr)
                call = subprocess.run(cmds[1].split(), input=b'{"status":"ok"}\n', capture_output=True)
                self.assertEqual(call.returncode, 0, call.stderr)
                self.assertEqual(call.stdout.strip(), b'{"status":"ok"}')
            finally:
                subprocess.run(cmds[2].split(), check=True)

    def test_remote_check_returns_a_correctable_diagnostic(self):
        with patch.object(remote, "_call", side_effect=RuntimeError("Clé inconnue.")):
            result = remote.preflight({"host": "music.test"}, {})
        self.assertFalse(result["ok"])
        self.assertEqual(result["checks"][0]["id"], "ssh")

    def test_tunnel_failure_preserves_success_and_explains_how_to_reconnect(self):
        ssh = {"host": "test.invalid", "port": 22, "user": "music"}
        remote_result = {"url": "http://127.0.0.1:9880/setup", "directory": "/music", "lanPending": True}
        with patch.object(remote, "_call", return_value=remote_result), patch.object(remote, "_client", return_value=["ssh"]), \
                patch.object(remote.subprocess, "Popen") as proc, patch.object(remote, "_tunnel_ready", return_value=False), \
                patch.object(remote.socket, "socket") as candidate:
            candidate.return_value.__enter__.return_value.getsockname.return_value = ("127.0.0.1", 1234)
            proc.return_value.poll.return_value = None
            result = remote.RemoteInstaller().run(ssh, "install", {"port": 9880}, lambda *_: None)
        self.assertEqual(result["url"], "")
        self.assertTrue(result["tunnelError"])
        self.assertTrue(result["setupManualUrl"].endswith("/setup"))
        self.assertIn(":127.0.0.1:9880", result["setupTunnel"])
        proc.return_value.terminate.assert_called_once()


if __name__ == "__main__":
    unittest.main()
