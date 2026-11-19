import os
import unittest
from unittest.mock import patch

from installer import i18n, launcher


class ConsoleEntryTest(unittest.TestCase):
    def test_language_matches_application_locales_and_regional_preferences(self):
        self.assertEqual(set(i18n.LANGUAGES), {"en", "es", "fr", "ru", "zh"})
        with patch.dict(os.environ, {"LC_MESSAGES": "fr_CA.UTF-8"}, clear=True):
            self.assertEqual(i18n.language(), "fr")
            self.assertEqual(i18n.language("zh-Hans-CN"), "zh")

    def test_arguments_reach_cli_without_starting_a_server(self):
        with patch.object(launcher.cli, "main", return_value=2) as console:
            self.assertEqual(launcher.main(["--lang", "ru", "check"]), 2)
        console.assert_called_once_with(["--lang", "ru", "check"])
