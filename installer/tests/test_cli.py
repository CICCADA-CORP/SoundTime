import contextlib
import io
import json
import os
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

from installer import cli, i18n, onboarding
from installer.cli_strings import ROWS


def invoke(args):
    stdout, stderr = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
        result = cli.main(args)
    return result, stdout.getvalue(), stderr.getvalue()


class TerminalTests(unittest.TestCase):
    def test_translated_help_for_every_command(self):
        for lang in i18n.LANGUAGES:
            for command in ("check", "install", "setup", "enable-lan", "export", "wizard"):
                code, output, _ = invoke([command, "--lang", lang, "--help"])
                with self.subTest(lang=lang, command=command):
                    self.assertEqual(code, 0)
                    self.assertIn(i18n.text(lang, ROWS, command).split()[0], output)
                    self.assertIn(i18n.text(lang, ROWS, "usage"), output)
        self.assertTrue(all(len(line) == len(i18n.LANGUAGES) and all(line) for line in ROWS.values()))

    def test_bad_settings_are_reported_as_one_json_failure(self):
        code, output, _ = invoke(["--json", "check", "--port", "0", "--lang", "es"])
        self.assertEqual(code, 1)
        response = json.loads(output)
        self.assertFalse(response["ok"])
        self.assertEqual(response["checks"][0]["id"], "settings")
        self.assertNotIn("Choisissez", output)

    def test_private_values_never_enter_diagnostics(self):
        secret = "highly-sensitive-placeholder"
        code, output, error = invoke(["--lang", "en", "install", "--json", "--s3-secret-key", secret])
        self.assertEqual(code, 2)
        self.assertFalse(json.loads(output)["ok"])
        self.assertNotIn(secret, output + error)

    def test_archive_uses_env_keys_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "project.zip"
            args = ["export", "--json", "--target", "nas", "--directory", "/volume1/docker/SoundTime",
                    "--storage", "s3", "--s3-bucket", "music-library", "--lang", "es", "--output", str(path)]
            secret = "sample$'\\secret"
            with patch.dict(os.environ, {"SOUNDTIME_S3_ACCESS_KEY": "sample-key", "SOUNDTIME_S3_SECRET_KEY": secret}):
                code, text, error = invoke(args)
                self.assertEqual(code, 0, text + error)
                self.assertEqual(json.loads(text)["archive"], str(path))
                self.assertNotIn(secret, text + error)
                original = path.read_bytes()
                with zipfile.ZipFile(path) as z:
                    self.assertIn("compose.yaml", z.namelist())
                    self.assertIn("qnap-compose.yaml", z.namelist())
                    self.assertIn("README.txt", z.namelist())
                if os.name != "nt":
                    self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                self.assertEqual(invoke(args)[0], 1)
                self.assertEqual(path.read_bytes(), original)

    def test_noninteractive_missing_s3_credentials_is_an_error(self):
        with patch.dict(os.environ, {}, clear=True):
            result, output, _ = invoke(["--json", "check", "--storage", "s3", "--lang", "en"])
        self.assertEqual(result, 1)
        self.assertIn("SOUNDTIME_S3_ACCESS_KEY", json.loads(output)["error"])

    def test_tunnel_closes_and_reconnect_command_is_retained(self):
        deployed = {"directory": "/data/st", "url": "http://127.0.0.1:6001/setup", "lanPending": True,
                    "ssh": True, "setupTunnel": "ssh -N -L ...", "setupManualUrl": "http://127.0.0.1:6001/setup"}
        with patch.object(cli.remote, "detect", return_value={"os": "linux", "arch": "x64"}) as detect, \
                patch.object(cli.remote, "recommend", return_value={**cli.engine.defaults(), "directory": "/data/st"}), \
                patch.object(cli.remote, "RemoteInstaller") as client:
            client.return_value.run.return_value = deployed
            code, output, _ = invoke(["install", "--ssh-host", "example.test", "--ssh-user", "music",
                                      "--directory", "/data/st", "--access", "lan", "--json"])
            self.assertEqual(code, 0, output)
            detect.assert_called_once()
            client.return_value.close.assert_called_once()
            result = json.loads(output)
            self.assertFalse(result["tunnelOpen"])
            self.assertFalse(result["url"])
            self.assertEqual(result["setupTunnel"], deployed["setupTunnel"])
            self.assertTrue(result["lanPending"])

    def test_headless_resume_preserves_managed_s3_and_custom_ports(self):
        saved = {**cli.engine.defaults(), "directory": "/library", "port": 8894, "p2p": True,
                 "storage": {"kind": "s3", "reuseCredentials": True, "region": "auto",
                             "bucket": "music", "endpoint": "https://obj.example", "prefix": "media"}}
        with patch.object(cli.engine, "recommend", return_value=saved) as recommend, \
                patch.object(cli.engine, "install", return_value={"url": "http://127.0.0.1:8894/"}) as install:
            self.assertEqual(invoke(["install", "--json", "--directory", "/library"])[0], 0)
        recommend.assert_called_once_with("/library")
        sent = install.call_args.args[0]
        self.assertEqual(sent, saved)
        self.assertTrue(sent["storage"]["reuseCredentials"])

    def test_wizard_first_question_and_cancel_before_writes(self):
        prompts = []
        def response(prompt):
            prompts.append(prompt)
            return "" if len(prompts) < 3 else "2"
        checks = {"ok": True, "checks": []}
        with patch("builtins.input", side_effect=response), patch.object(cli.engine, "recommend", return_value=cli.engine.defaults()), \
                patch.object(cli.engine, "preflight", return_value=checks), \
                patch.object(cli.engine, "install") as install:
            code, text, _ = invoke(["--lang", "fr"])
        self.assertEqual(code, 0)
        self.assertIn("Où souhaitez-vous installer SoundTime", text.splitlines()[1])
        self.assertIn("Sur une autre machine", text)
        install.assert_not_called()

    def test_remote_wizard_detects_host_before_preflight(self):
        replies = ["2", "1", "win.example.test", "music", "2222", "1", "2"]
        suggested = {**cli.engine.defaults(), "directory": "C:\\SoundTime", "port": 8892}
        with patch("builtins.input", side_effect=replies), \
                patch.object(cli.remote, "detect", return_value={"os": "windows", "arch": "arm64"}) as detect, \
                patch.object(cli.remote, "recommend", return_value=suggested), \
                patch.object(cli.remote, "preflight", return_value={"ok": True, "checks": []}) as checks:
            code, output, _ = invoke(["wizard", "--lang", "en"])
        self.assertEqual(code, 0, output)
        self.assertIn("Windows · arm64", output)
        detect.assert_called_once()
        self.assertEqual(checks.call_args.args[0]["os"], "windows")
        self.assertEqual(checks.call_args.args[1]["directory"], "C:\\SoundTime")
        self.assertEqual(checks.call_args.args[1]["port"], 8892)

    def test_setup_uses_loopback_port_and_protected_credentials(self):
        with patch.dict(os.environ, {"SOUNDTIME_ADMIN_PASSWORD": "test-password"}), \
                patch.object(cli.onboarding, "configure", side_effect=lambda port, load, lang, **_:
                    {"setupComplete": load()["username"] == "music", "alreadyComplete": False}) as setup:
            code, output, _ = invoke(["setup", "--port", "9123", "--admin-user", "music", "--admin-email", "a@b.test", "--json"])
            self.assertTrue(json.loads(output)["setupComplete"])
            self.assertEqual(code, 0)
            self.assertNotIn("test-password", output)
            self.assertEqual(setup.call_args.args[0], 9123)

    def test_detected_nas_offers_vendor_project_without_deploying_agent(self):
        with tempfile.TemporaryDirectory() as folder:
            zip_path = str(Path(folder) / "nas.zip")
            replies = ["2", "1", "nas.home", "music", "22", "1", "1", zip_path, "1"]
            with patch("builtins.input", side_effect=replies), \
                    patch.object(cli.remote, "detect", return_value={"os": "linux", "arch": "x64", "nas": "qnap"}), \
                    patch.object(cli.remote, "recommend") as recommend, patch.object(cli.remote, "preflight") as check:
                code, output, _ = invoke(["wizard", "--lang", "en"])
            self.assertEqual(code, 0, output)
            self.assertIn("qnap-compose.yaml", output)
            self.assertIn("/share/Container/soundtime", output)
            self.assertTrue(Path(zip_path).is_file())
            recommend.assert_not_called()
            check.assert_not_called()


class OnboardingTests(unittest.TestCase):
    def setUp(self):
        self.account = {"username": "admin", "email": "admin@example.test", "password": "sample-secret",
                        "instanceName": "SoundTime QA"}

    def test_private_setup_then_idempotent_retry(self):
        state = {"has_admin": False, "setup_complete": False}
        calls = []
        def request(port, path, payload=None, headers=None):
            self.assertEqual(port, 8880)
            calls.append(path)
            if path == "/api/setup/status":
                return 200, dict(state), []
            if path == "/api/setup/admin":
                state["has_admin"] = True
                return 201, {"tokens": {"access_token": "test-token"}}, []
            self.assertEqual(headers.get("Authorization"), "Bearer test-token")
            if path == "/api/setup/complete":
                self.assertTrue(payload["instance_private"])
                self.assertFalse(payload["open_registrations"])
                state["setup_complete"] = True
            return 200, {}, []

        with patch.object(onboarding, "_request", side_effect=request):
            first = onboarding.configure(8880, self.account)
            second = onboarding.configure(8880, lambda: self.fail("already complete must not request credentials"))
        self.assertTrue(first["setupComplete"])
        self.assertFalse(first["alreadyComplete"])
        self.assertTrue(second["alreadyComplete"])
        self.assertEqual(calls.count("/api/setup/admin"), 1)
        self.assertNotIn("test-token", repr(first))

    def test_resume_partial_setup_logs_in_without_creating_a_second_user(self):
        responses = [(200, {"has_admin": True, "setup_complete": False}, []),
                     (200, {"tokens": {"access_token": "existing-token"}}, []),
                     (200, {}, []), (200, {}, []),
                     (200, {"has_admin": True, "setup_complete": True}, [])]
        with patch.object(onboarding, "_request", side_effect=responses) as http:
            result = onboarding.configure(8880, self.account)
        self.assertTrue(result["setupComplete"])
        self.assertEqual(http.call_args_list[1].args[1], "/api/auth/login")
        self.assertNotIn("/api/setup/admin", [c.args[1] for c in http.call_args_list])

    def test_invalid_destination_and_non_soundtime_server_never_receive_password(self):
        for port in ("https://example.org", 0, True):
            with patch.object(onboarding, "_request") as http:
                with self.assertRaises(ValueError):
                    onboarding.configure(port, self.account)
                http.assert_not_called()
        with patch.object(onboarding, "_request", return_value=(200, {"foreign": True}, [])) as http, \
                self.assertRaisesRegex(RuntimeError, "SoundTime"):
            onboarding.configure(8880, self.account)
        self.assertEqual(http.call_count, 1)
        self.assertIsNone(http.call_args.args[2])


if __name__ == "__main__":
    unittest.main()
