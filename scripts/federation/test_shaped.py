"""Classify the controlled-link results without running a network generator."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import shaped


class ShapedVerdict(unittest.TestCase):
    def run_cli(self, benchmark):
        with tempfile.TemporaryDirectory() as directory:
            outfile = Path(directory) / "report.json"
            args = ["shaped", "--mode", "shaped", "--a", "a", "--b", "b", "--c", "c", "--report", str(outfile)]
            with patch.object(sys, "argv", args), patch.object(shaped, "benchmark", side_effect=benchmark):
                code = shaped.main()
            return code, json.loads(outfile.read_text())

    def test_measured_target_is_strict_and_retains_the_sample(self):
        for p95, code, verdict in ((1999.9, 0, "passed"), (2000, 1, "not_met"), (2001, 1, "not_met")):
            with self.subTest(p95=p95):
                def measured(args, report):
                    report["first_sound"] = {"warm": {"p95_ms": p95, "samples": 20}}
                result, report = self.run_cli(measured)
                self.assertEqual((result, report["status"], report["verdict"]), (code, "measured", verdict))
                self.assertEqual(report["first_sound"]["warm"]["p95_ms"], p95)

    def test_unusable_link_is_separate_from_incomplete_playback(self):
        for phase, verdict in (("calibration", "invalid_calibration"), ("playback", "insufficient_evidence")):
            with self.subTest(phase=phase):
                def incomplete(args, report):
                    report["phase"] = phase
                    raise ValueError("witness unavailable")
                with patch("sys.stderr"):
                    result, report = self.run_cli(incomplete)
                self.assertEqual((result, report["status"], report["verdict"]), (1, "failed", verdict))
                self.assertEqual(report["error"], "witness unavailable")
                self.assertNotIn("warm_p95_under_2s", report)


if __name__ == "__main__":
    unittest.main()
