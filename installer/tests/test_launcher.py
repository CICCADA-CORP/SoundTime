import contextlib
import io
import os
import unittest
from unittest.mock import patch

from installer import launcher


class ConsoleEntryTest(unittest.TestCase):
    def test_help_is_english_on_a_french_machine_even_with_localized_argparse(self):
        with patch.dict(os.environ, {"LC_MESSAGES": "fr_CA.UTF-8", "LANGUAGE": "fr", "SOUNDTIME_LANG": "fr"}), \
                patch("argparse._", side_effect=lambda s: "Utilisation : " if s == "usage: " else s):
            for args in (["--help"], ["--lang=fr_CA", "--help"], ["--lang", "fr", "--help"]):
                with self.subTest(args=args), contextlib.redirect_stdout(io.StringIO()) as stdout:
                    self.assertEqual(launcher.main(args), 0)
                    output = stdout.getvalue()
                    self.assertTrue(output.startswith("Usage: soundtime"))
                    self.assertIn("Install SoundTime", output)
                    self.assertNotIn("--lang", output)

    def test_arguments_reach_cli_without_starting_a_server(self):
        with patch.object(launcher.cli, "main", return_value=2) as console:
            self.assertEqual(launcher.main(["--lang", "ru", "check"]), 2)
        console.assert_called_once_with(["--lang", "ru", "check"])
