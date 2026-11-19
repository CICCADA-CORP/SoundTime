import json
import base64
import hashlib
import io
import os
import re
import shutil
import socket
import tempfile
import unittest
import zipfile
from pathlib import Path
from subprocess import CompletedProcess
from unittest.mock import Mock, patch

from installer import engine


class InstallationTests(unittest.TestCase):
    S3 = {
        "kind": "s3", "region": "eu-west-3", "bucket": "soundtime-music",
        "endpoint": "", "prefix": "collection/${MUSIC}",
        "accessKey": "my-key", "secretKey": "a'\\\"${HOME}# +/=\\",
    }
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.directory = Path(temp.name).resolve() / "Ma musique"
        self.settings = {**engine.defaults(), "directory": str(self.directory)}

    def prepare(self):
        with engine._lock(self.directory):
            state = engine._prepare(self.directory)
            engine._configure(self.directory, self.settings)
            return state

    def test_input_validation(self):
        for key, value in (("directory", "../lib"), ("port", True), ("port", 65536),
                           ("peerPort", 0), ("domain", "${SECRET}"), ("p2p", 1),
                           ("domain", "https://music.lan"), ("access", "public")):
            with self.subTest(key=key, value=value):
                response = engine.preflight({**self.settings, key: value})
                self.assertFalse(response["ok"])
                self.assertEqual(response["checks"][0]["id"], "settings")

    def test_other_installation_preserved(self):
        self.directory.mkdir()
        env = self.directory / ".env"
        env.write_text("JWT_SECRET=custom\n")
        with self.assertRaises(ValueError):
            self.prepare()
        self.assertEqual(env.read_text(), "JWT_SECRET=custom\n")
        self.assertEqual(list(self.directory.iterdir()), [env])

    def test_music_source_must_be_separate_and_present(self):
        for music in (".", str(self.directory.parent), self.directory.anchor,
                      str(self.directory / "docker"), str(self.directory.parent / "unplugged")):
            with self.subTest(music=music), self.assertRaises(ValueError):
                engine._settings({**self.settings, "musicDirectory": music})
        older = self.settings.copy()
        older.pop("musicDirectory")
        self.assertEqual(engine._settings(older)["musicDirectory"], "")

    def test_invalid_s3_options_do_not_disclose_credentials(self):
        for key, invalid in (("endpoint", "https://user:password@bucket.invalid"), ("endpoint", "file:///volume1"),
                             ("bucket", "../library"), ("region", ""), ("secretKey", "abc\ndef"),
                             ("endpoint", "https://minio.invalid:65536"), ("prefix", "foo/../private")):
            with self.subTest(key=key), self.assertRaises(ValueError) as caught:
                engine._settings({**self.settings, "storage": {**self.S3, key: invalid}})
            self.assertNotIn(self.S3["secretKey"], str(caught.exception))
        with self.assertRaisesRegex(ValueError, "ou le bucket S3"):
            engine._settings({**self.settings, "musicDirectory": str(self.directory), "storage": self.S3})

    @unittest.skipUnless(shutil.which("docker"), "Docker Compose CLI required")
    def test_s3_runtime_env_and_credentials_after_reconfiguration(self):
        owner = self.prepare()
        envfile = self.directory / ".env"
        original = engine._unseal(envfile)
        for endpoint in ("", "https://account.r2.cloudflarestorage.com", "http://minio.local:9000"):
            settings = engine._settings({**self.settings, "storage": {**self.S3, "endpoint": endpoint}})
            engine._configure(self.directory, settings)
            self.assertEqual(engine._state(self.directory), owner)
            self.assertTrue(engine._unseal(envfile).startswith(original))
            self.assertNotIn(self.S3["accessKey"], (self.directory / "compose.override.yaml").read_text())
            self.assertNotIn(self.S3["secretKey"], json.dumps(engine._result(settings, {"setup_complete": False})))
            result = engine._docker("compose", "config", "--format", "json", cwd=self.directory)
            self.assertEqual(result.returncode, 0)
            env = json.loads(result.stdout)["services"]["backend"]["environment"]
            self.assertEqual(env["STORAGE_BACKEND"], "s3")
            self.assertEqual(env["S3_ACCESS_KEY"], self.S3["accessKey"])
            self.assertEqual(env["S3_SECRET_KEY"].replace("$$", "$"), self.S3["secretKey"])
            self.assertEqual(env["S3_PREFIX"].replace("$$", "$"), self.S3["prefix"])
            self.assertEqual(env.get("S3_ENDPOINT"), endpoint or None)
        engine._configure(self.directory, self.settings)
        self.assertEqual(envfile.read_bytes(), engine._seal(original))

    def test_portable_project_exposes_federation_controls(self):
        settings = {**self.settings, "directory": "/volume1/docker/soundtime", "p2p": True, "peerPort": 22000}
        _, contents = engine.export_project(settings, "nas")
        with zipfile.ZipFile(io.BytesIO(contents)) as archive:
            env = json.loads(archive.read("compose.yaml"))["services"]["backend"]["environment"]
            self.assertEqual(env["P2P_ENABLED"], "true")
            self.assertEqual(env["P2P_BIND_PORT"], "22000")
            self.assertEqual(env["P2P_EXPLICIT_CATALOG"], "${P2P_EXPLICIT_CATALOG:-false}")
            self.assertEqual(env["P2P_CACHE_MAX_SIZE"], "${P2P_CACHE_MAX_SIZE:-2GB}")
            self.assertEqual(env["P2P_PEER_UPLOAD_BYTES_PER_SECOND"], "${P2P_PEER_UPLOAD_BYTES_PER_SECOND:-0}")
            self.assertIn('P2P_CACHE_MAX_SIZE="2GB"', archive.read("qnap/backend.env").decode())

    def test_portable_project_can_be_generated_without_remote_files_or_docker(self):
        settings = {**self.settings, "directory": "/volume1/docker/ma musique", "musicDirectory": "/volume2/Albums"}
        for target in ("nas", "server", "vps", "vm", "cloud"):
            with self.subTest(target=target), patch.object(engine, "_docker", side_effect=AssertionError):
                name, data = engine.export_project(settings, target)
                self.assertEqual(name, f"soundtime-{target}.zip")
                with zipfile.ZipFile(io.BytesIO(data)) as zipdata:
                    self.assertEqual(zipdata.getinfo(".env").external_attr >> 16 & 0o777, 0o600)
                    model = json.loads(zipdata.read("compose.yaml"))
                    self.assertEqual(len(model["services"]), 5)
                    self.assertEqual(model["services"]["nginx"]["ports"][0]["host_ip"], "${SOUNDTIME_BIND_IP:-127.0.0.1}")
                    for svc in ("backend", "postgres", "redis", "frontend"):
                        self.assertFalse(model["services"][svc].get("ports"))
                    mount = next(m for m in model["services"]["backend"]["volumes"] if isinstance(m, dict))
                    self.assertTrue(mount["read_only"])
                    self.assertEqual(mount["source"], "/volume2/Albums")
                    self.assertNotIn("docker/acces/comptes", zipdata.namelist())
        for bad in ("/", "/opt/../etc", "C:\\Music", "relative/config"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                engine.export_project({**settings, "directory": bad}, "cloud")

    @unittest.skipUnless(shutil.which("docker"), "Docker Compose CLI required")
    def test_nas_lan_bootstrap_and_exported_s3_project_resolve(self):
        settings = {**self.settings, "directory": "/volume1/docker/test-soundtime", "access": "lan", "storage": self.S3}
        _, data = engine.export_project(settings, "nas")
        self.directory.mkdir(parents=True)
        with zipfile.ZipFile(io.BytesIO(data)) as zipdata:
            zipdata.extractall(self.directory)
            guide = zipdata.read("LISEZ-MOI.txt").decode()
            password = re.search(r"Mot de passe temporaire : (\S+)", guide)[1]
            digest = base64.b64encode(hashlib.sha1(password.encode()).digest()).decode()
            self.assertEqual(zipdata.read("docker/acces/comptes").decode(), f"soundtime:{{SHA}}{digest}\n")
            self.assertIn(b"auth_basic $soundtime_realm", zipdata.read("docker/acces/bootstrap.active"))
            self.assertNotIn(password, zipdata.read("compose.yaml").decode())
            self.assertNotIn(self.S3["secretKey"], guide)
        result = engine._docker("compose", "config", "--format", "json", cwd=self.directory)
        self.assertEqual(result.returncode, 0)
        services = json.loads(result.stdout)["services"]
        self.assertEqual(services["nginx"]["ports"][0]["host_ip"], "0.0.0.0")
        env = services["backend"]["environment"]
        self.assertEqual(env["S3_SECRET_KEY"].replace("$$", "$"), self.S3["secretKey"])
        self.assertNotIn("S3_ENDPOINT", env)

    def test_resume_keeps_project_credentials_and_manual_files(self):
        state = self.prepare()
        env = self.directory / ".env"
        original = env.read_bytes()
        settings = self.settings.copy()
        settings.update(port=9000, p2p=True)
        with engine._lock(self.directory):
            self.assertEqual(engine._prepare(self.directory), state)
            engine._configure(self.directory, settings)
        self.assertEqual(env.read_bytes(), original)
        variables = dict(line.split("=", 1) for line in env.read_text().splitlines() if "=" in line and not line.startswith("#"))
        self.assertRegex(variables["POSTGRES_PASSWORD"], r"^[a-f0-9]{64}$")
        self.assertNotEqual(variables["POSTGRES_PASSWORD"], variables["JWT_SECRET"])
        if os.name != "nt":
            self.assertEqual(env.stat().st_mode & 0o777, 0o600)
        override = self.directory / "compose.override.yaml"
        override.write_text(override.read_text() + "# custom\n")
        with self.assertRaisesRegex(ValueError, "modifié"):
            engine._configure(self.directory, self.settings)
        self.assertTrue(override.read_text().endswith("# custom\n"))

    def test_missing_credentials_do_not_recreate_a_database_password(self):
        self.prepare()
        (self.directory / ".env").unlink()
        with self.assertRaises(ValueError):
            self.prepare()

    def test_two_launchers_cannot_use_the_same_directory_at_once(self):
        with engine._lock(self.directory):
            with self.assertRaisesRegex(RuntimeError, "autre installation"):
                with engine._lock(self.directory):
                    self.fail("Le verrou aurait dû bloquer la seconde opération.")

    @unittest.skipUnless(shutil.which("docker"), "Docker Compose CLI required")
    def test_real_compose_merge_only_publishes_explicit_ports(self):
        state = self.prepare()
        for p2p, lan in ((False, False), (True, False), (True, True)):
            with self.subTest(p2p=p2p, lan=lan):
                settings = {**self.settings, "p2p": p2p, "access": "lan" if lan else "local"}
                engine._configure(self.directory, settings, lan)
                result = engine._docker("compose", "config", "--format", "json", cwd=self.directory)
                self.assertEqual(result.returncode, 0, result.stderr)
                config = json.loads(result.stdout)
                self.assertEqual(config["name"], state["project"])
                services = config["services"]
                self.assertNotIn("container_name", services["redis"])
                for service in ["redis", "postgres", "frontend"]:
                    self.assertFalse(services[service].get("ports"))
                web, = services["nginx"]["ports"]
                self.assertEqual((web["host_ip"], web["published"], web["target"]),
                                 ("0.0.0.0" if lan else "127.0.0.1", "8880", 80))
                peer = services["backend"].get("ports", [])
                self.assertEqual(len(peer), int(p2p))
                if p2p:
                    self.assertEqual((peer[0]["protocol"], peer[0]["published"]), ("udp", "11204"))
                self.assertEqual(services["backend"]["environment"]["P2P_ENABLED"], str(p2p).lower())
                pg = services["postgres"]["environment"]["POSTGRES_PASSWORD"]
                self.assertIn(pg, services["backend"]["environment"]["DATABASE_URL"])

    @unittest.skipUnless(shutil.which("docker"), "Docker Compose CLI required")
    def test_collection_path_is_literal_and_read_only(self):
        quote = "'" if os.name == "nt" else '"'
        music = self.directory.parent / (f"Les {quote}disques{quote} " + "${HOME} à moi")
        music.mkdir()
        self.prepare()
        settings = engine._settings({**self.settings, "musicDirectory": str(music)})
        engine._configure(self.directory, settings)
        result = engine._docker("compose", "config", "--format", "json", cwd=self.directory)
        self.assertEqual(result.returncode, 0, result.stderr)
        mounts = json.loads(result.stdout)["services"]["backend"]["volumes"]
        source, = [vol for vol in mounts if vol["type"] == "bind"]
        # "compose config" re-escapes literal dollars when serializing a model.
        self.assertEqual(Path(source["source"].replace("$$", "$")), music)
        self.assertEqual(source["target"], "/data/music/collection")
        self.assertTrue(source["read_only"])
        self.assertFalse(source.get("bind", {}).get("create_host_path", False))
        self.assertEqual({v["source"] for v in mounts if v["type"] == "volume"},
                         {"music_data", "p2p_data"})
        self.assertEqual(engine._result(settings, {"setup_complete": False})["musicDirectory"], str(music))

    def test_cloud_or_windows_docker_refused(self):
        with patch.dict(os.environ, {"DOCKER_HOST": "ssh://another-machine", "DOCKER_CONTEXT": ""}), patch("shutil.which", return_value="docker"):
            with self.assertRaisesRegex(ValueError, "distant"):
                engine._docker_ready()
        with patch.dict(os.environ, {"DOCKER_HOST": "unix:///var/run/docker.sock", "DOCKER_CONTEXT": ""}), patch("shutil.which", return_value="docker"), patch.object(engine, "_docker", return_value=CompletedProcess([], 0, "windows/amd64", "")):
            with self.assertRaisesRegex(ValueError, "conteneurs Linux"):
                engine._docker_ready()

    def test_preflight_occupied_port_can_be_retried(self):
        with patch.object(engine, "_docker_ready", return_value="Docker disponible"):
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
                listener.bind(("127.0.0.1", 0))
                listener.listen()
                settings = {**self.settings, "port": listener.getsockname()[1]}
                response = engine.preflight(settings)
                self.assertFalse(response["ok"])
                self.assertEqual(response["checks"][-1]["id"], "ports")
            self.assertTrue(engine.preflight(settings)["ok"])
            self.assertFalse(self.directory.exists())

    def test_lan_requires_finished_onboarding_and_owned_port(self):
        settings = {**self.settings, "access": "lan"}
        reports = []
        status = {"has_admin": False, "setup_complete": False}
        with patch.object(engine, "_validate_preflight"), patch.object(engine, "_compose"), patch.object(engine, "_download"), patch.object(engine, "_docker_ready"), patch.object(engine, "_readiness", return_value=status):
            result = engine.install(settings, lambda stage, _: reports.append(stage))
            self.assertEqual(reports, ["prepare", "download", "start", "verify"])
            self.assertTrue(result["lanPending"])
            self.assertTrue(result["url"].endswith("/setup"))
            with patch.object(engine, "_port_owner", return_value=set()):
                with self.assertRaisesRegex(ValueError, "changé"):
                    engine.enable_lan(settings, Mock())
            with patch.object(engine, "_port_owner", return_value={settings["port"]}):
                with self.assertRaisesRegex(RuntimeError, "Terminez"):
                    engine.enable_lan(settings, Mock())
                self.assertNotIn("0.0.0.0", (self.directory / "compose.override.yaml").read_text())
                status.update(has_admin=True, setup_complete=True)
                with patch.object(engine, "_lan_host", return_value="192.168.1.20"):
                    result = engine.enable_lan(settings, Mock())
                self.assertFalse(result["lanPending"])
                self.assertEqual(result["lanUrl"], "http://192.168.1.20:8880/")

    def test_download_preserves_cached_latest_and_pulls_missing_image(self):
        calls = []

        def docker(*args, **kwargs):
            calls.append(args)
            missing = args[0] == "image" and args[-1] == "redis:7-alpine"
            return CompletedProcess(args, int(missing), "", "")

        with patch.object(engine, "_compose", return_value="soundtime:latest\nredis:7-alpine\n"), patch.object(engine, "_docker", side_effect=docker):
            engine._download(self.directory, Mock())
        self.assertEqual([args for args in calls if args[0] == "pull"],
                         [("pull", "--quiet", "redis:7-alpine")])

    def test_readiness_requires_database_and_frontend(self):
        for replies in (
            [{"status": "ok"}, {}],
            [{"status": "ok"}, {"has_admin": True, "setup_complete": True}, OSError("frontend loading")],
        ):
            with patch.object(engine, "_get", side_effect=replies):
                with self.assertRaises(RuntimeError):
                    engine._readiness(8880, wait=0)


if __name__ == "__main__":
    unittest.main()
