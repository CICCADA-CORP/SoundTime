import errno
import io
import os
import unittest
import zipfile
from string import Formatter
from unittest.mock import patch

from installer import diagnostics, engine, remote
from installer.diagnostic_strings import LEGACY_FRENCH
from installer.guide_strings import SECTIONS


class OperatorLanguageTests(unittest.TestCase):
    def test_legacy_agent_diagnostics_are_read_in_english(self):
        values = {"host": "studio.example", "name": "/Music/Été {1}", "port": 8882,
                  "version": "v2.39.4", "i": 2, "total": 5, "image": "example/db:pg16"}
        for french, english in LEGACY_FRENCH.items():
            with self.subTest(message=french):
                self.assertEqual({f for _, f, _, _ in Formatter().parse(french) if f},
                                 {f for _, f, _, _ in Formatter().parse(english) if f})
                self.assertEqual(diagnostics.localize("fr", french.format(**values)), english.format(**values))
                self.assertEqual(diagnostics.english(english.format(**values)), english.format(**values))
        unknown = "[Errno 13] /a/{some-file}: read failed"
        self.assertEqual(diagnostics.english(unknown), unknown)

    def test_native_diagnostics_are_english_on_a_french_machine(self):
        with patch.dict(os.environ, {"LANG": "fr_FR.UTF-8", "SOUNDTIME_LANG": "fr"}):
            check, = engine.preflight({"port": 0})["checks"]
            self.assertEqual(check["label"], "Settings")
            self.assertIn("Some settings are missing", check["detail"])
            exc = OSError(errno.EACCES, "Permission refusée", "/Music/Été")
            with patch.object(engine, "_settings", side_effect=exc):
                check, = engine.preflight({})["checks"]
            self.assertIn("Permission denied", check["detail"])
            self.assertIn("/Music/Été", check["detail"])

    def test_legacy_remote_check_is_english_without_changing_metadata(self):
        reply = {"ok": False, "checks": [{"id": "ports", "label": "Ports réseau", "ok": False,
                                         "detail": "Le port 8882 est occupé ou réservé. Choisissez un autre numéro dans les réglages."}]}
        with patch.object(remote, "_call", return_value=reply):
            result = remote.preflight({"host": "music.example"}, {})
        self.assertEqual(result["checks"][0]["label"], "Server connection")
        failure = result["checks"][1]
        self.assertEqual((failure["label"], failure["id"], failure["ok"]), ("Network ports", "ports", False))
        self.assertIn("Port 8882 is in use", failure["detail"])

    def test_generated_guides_are_in_english_for_every_target(self):
        for target in ("nas", "server", "vm", "vps", "cloud"):
            with self.subTest(target=target), patch.dict(os.environ, {"LANG": "fr_FR.UTF-8", "SOUNDTIME_LANG": "fr"}):
                settings = dict(engine.defaults(), directory="/srv/SoundTime {test}", access="lan",
                                musicDirectory="/media/My $Albums", port=8883)
                _, data = engine.export_project(settings, target, language="fr")
                with zipfile.ZipFile(io.BytesIO(data)) as project:
                    guides = [name for name in project.namelist() if name.endswith(".txt")]
                    self.assertEqual(guides, ["README.txt"])
                    guide = project.read("README.txt").decode()
                    self.assertEqual(project.getinfo("README.txt").external_attr >> 16 & 0o777, 0o600)
                    if target == "nas":
                        self.assertIn("docker/access/bootstrap.active", project.namelist())
                        self.assertIn("docker/access/bootstrap.active", guide)
                        self.assertIn("bootstrap.done", guide)
                        self.assertIn("Temporary username: soundtime", guide)
                    else:
                        self.assertIn("ssh -N -L 127.0.0.1:8883:127.0.0.1:8883", guide)
                self.assertIn("Create the first account", guide)
                self.assertIn(SECTIONS["title_" + target], guide)
                self.assertIn(SECTIONS["collection"].format(folder=settings["musicDirectory"]), guide)
                self.assertIn("8883/setup", guide)
