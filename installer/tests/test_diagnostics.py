import io
import unittest
import zipfile
from string import Formatter

from installer import diagnostics, engine, i18n
from installer.diagnostic_strings import MESSAGES
from installer.guide_strings import ROWS


class OperatorLanguageTests(unittest.TestCase):
    def test_all_diagnostics_preserve_their_values(self):
        values = {"host": "studio.example", "name": "/Music/Été {1}", "port": 8882,
                  "version": "v2.39.4", "i": 2, "total": 5, "image": "example/db:pg16"}
        self.assertEqual(len(MESSAGES), len({row[1] for row in MESSAGES}))
        for row in MESSAGES:
            self.assertEqual(len(row), len(i18n.LANGUAGES))
            fields = {field for _, field, _, _ in Formatter().parse(row[1]) if field}
            source = row[1].format(**values)
            for code, template in zip(i18n.LANGUAGES, row):
                with self.subTest(code=code, message=source):
                    self.assertEqual(fields, {f for _, f, _, _ in Formatter().parse(template) if f})
                    self.assertEqual(diagnostics.localize(code, source), template.format(**values))
        unknown = "[Errno 13] /a/{some-file}: read failed"
        self.assertEqual(diagnostics.localize("zh", unknown), unknown)

    def test_generated_guides_follow_the_selected_language(self):
        for code in i18n.LANGUAGES:
            for target in ("nas", "server", "vm", "vps", "cloud"):
                with self.subTest(code=code, target=target):
                    settings = dict(engine.defaults(), directory="/srv/SoundTime {test}", access="lan",
                                    musicDirectory="/media/My $Albums", port=8883)
                    _, data = engine.export_project(settings, target, language=code)
                    with zipfile.ZipFile(io.BytesIO(data)) as project:
                        name = "LISEZ-MOI.txt" if code == "fr" else "README.txt"
                        guide = project.read(name).decode()
                        self.assertEqual(guide, project.read("LISEZ-MOI.txt").decode())
                        self.assertEqual(project.getinfo(name).external_attr >> 16 & 0o777, 0o600)
                    self.assertIn(i18n.text(code, ROWS, "account"), guide)
                    self.assertIn(i18n.text(code, ROWS, "title_" + target), guide)
                    self.assertIn(i18n.text(code, ROWS, "collection", folder=settings["musicDirectory"]), guide)
                    self.assertIn("8883/setup", guide)
                    if target == "nas":
                        self.assertIn(i18n.text(code, ROWS, "login"), guide)
                    else:
                        self.assertIn("ssh -N -L 127.0.0.1:8883:127.0.0.1:8883", guide)
