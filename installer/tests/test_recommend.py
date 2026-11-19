import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from installer import engine


class RecommendationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.path = self.root / "SoundTime"
        self.settings = dict(engine.defaults(), directory=str(self.path), domain="music.lan",
                             port=9813, p2p=True, peerPort=11210, access="lan")

    def configure(self):
        with engine._lock(self.path):
            engine._prepare(self.path)
            engine._configure(self.path, self.settings, lan=self.settings["access"] == "lan")

    def test_new_directory_uses_first_available_port_without_creating_files(self):
        with patch.object(engine, "_check_port", side_effect=[ValueError(), ValueError(), None]) as check:
            plan = engine.recommend(self.path)
        self.assertEqual(plan["port"], 8882)
        self.assertEqual(plan["directory"], str(self.path))
        self.assertEqual([call.args[0] for call in check.call_args_list], [8880, 8881, 8882])
        self.assertFalse(self.path.exists())

    def test_all_candidate_ports_occupied_falls_back_to_failing_preflight(self):
        message = "Le port 8880 est occupé ou réservé. Choisissez un autre numéro dans les réglages."
        with patch.object(engine, "_docker_ready", return_value="Docker"), \
                patch.object(engine, "_check_port", side_effect=ValueError(message)) as check:
            plan = engine.recommend(self.path)
            self.assertEqual(check.call_count, 20)
            self.assertEqual(plan["port"], 8880)
            failure, = (entry for entry in engine.preflight(plan)["checks"] if not entry["ok"])
        self.assertEqual((failure["id"], failure["detail"]), ("ports", message))
        self.assertFalse(self.path.exists())

    def test_owned_media_install_recovers_settings_even_if_drive_is_unplugged(self):
        music = self.root / "${My Albums} été"
        music.mkdir()
        self.settings["musicDirectory"] = str(music)
        self.configure()
        music.rmdir()
        with patch.object(engine, "_check_port") as check:
            self.assertEqual(engine.recommend(self.path), self.settings)
            check.assert_not_called()
        with self.assertRaisesRegex(ValueError, "reconnectez"):
            engine._settings(engine.recommend(self.path))

    def test_owned_s3_install_recovers_public_fields_and_retains_credentials(self):
        public = dict(kind="s3", endpoint="https://store.example", region="us-west-2",
                      bucket="music-storage", prefix="unit/${MUSIC}")
        for endpoint in ("https://store.example", ""):
            with self.subTest(endpoint=endpoint):
                public["endpoint"] = endpoint
                self.settings["storage"] = dict(public, accessKey="private-access-id", secretKey="private-secret-key")
                self.configure()
                config = self.path / "compose.override.yaml"
                env = self.path / ".env"
                original = env.read_bytes()
                restored = engine.recommend(self.path)
                self.assertEqual(restored, {**self.settings, "storage": dict(public, reuseCredentials=True)})
                self.assertNotIn("private-access-id", json.dumps(restored))
                self.assertNotIn("private-secret-key", json.dumps(restored))
                restored["port"] += 1
                engine._configure(self.path, engine._settings(restored), lan=True)
                self.assertEqual(env.read_bytes(), original)
                self.assertEqual(engine.recommend(self.path), restored)
                override = config.read_bytes()
                different = {**restored, "storage": {**restored["storage"], "endpoint": "https://different.example"}}
                with self.assertRaisesRegex(ValueError, "clés"):
                    engine._configure(self.path, different)
                self.assertEqual(env.read_bytes(), original)
                self.assertEqual(config.read_bytes(), override)
                with self.assertRaisesRegex(ValueError, "clés"):
                    engine.export_project({**restored, "directory": "/srv/soundtime"}, "nas", language="en")

    def test_normalization_protects_foreign_and_unsupported_files(self):
        self.configure()
        config = self.path / "compose.override.yaml"
        modified = config.read_bytes() + b"# edited outside SoundTime\n"
        config.write_bytes(modified)
        with self.assertRaisesRegex(ValueError, "modifi"):
            engine.recommend(self.path)
        self.assertEqual(modified, config.read_bytes())
        unsupported = engine._seal(b"services: {}\n")
        config.write_bytes(unsupported)
        with self.assertRaisesRegex(ValueError, "automatiquement"):
            engine.recommend(self.path)
        self.assertEqual(unsupported, config.read_bytes())

    def test_docker_subprocess_does_not_inherit_installer_credentials(self):
        keys = ("SOUNDTIME_S3_ACCESS_KEY", "SOUNDTIME_S3_SECRET_KEY",
                "SOUNDTIME_ADMIN_PASSWORD", "SOUNDTIME_BOOTSTRAP_PASSWORD")
        with patch.dict(os.environ, dict.fromkeys(keys, "private")):
            self.assertTrue(set(keys).isdisjoint(engine._docker_env()))
