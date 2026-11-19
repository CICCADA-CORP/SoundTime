"""Generator input, progress and deadlines, without starting a fleet or sending HTTP."""
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import load_driver
from scenario import ScenarioError


PROJECT = "st-fed-1234567890abcdef"


class LoadDriver(unittest.TestCase):
    def test_location_and_routes(self):
        with patch.dict(os.environ, {"FEDERATION_BROWSER":"container"}, clear=True):
            self.assertEqual(load_driver.location(), "container")
            with patch.dict(os.environ, {"SOUNDTIME_LOAD_GENERATOR":"native"}):
                self.assertEqual(load_driver.location(), "native")
        command, stop = load_driver.commands(PROJECT, "container")
        self.assertEqual(command[-5:], ["exec", "-T", "load-generator", "node", "/opt/soundtime-browser/load-client.mjs"])
        self.assertEqual(stop[-4:], ["stop", "--timeout", "3", "load-generator"])
        self.assertIsNone(load_driver.commands(PROJECT, "native")[1])
        with self.assertRaises(ScenarioError):
            load_driver.commands("production", "container")

    def test_collects_only_observed_frames_and_bounds_wait(self):
        report = {}
        records = load_driver.Records(report)
        with self.assertRaisesRegex(ScenarioError, "timed out"):
            records.wait("steady", 0)
        frames = [{"phase":"started","runtime":{"location":"container"}}, {"phase":"steady"},
                  {"phase":"sample","open_media_responses":9}]
        stream = io.BytesIO(b"".join(json.dumps(f).encode()+b"\n" for f in frames))
        progress = io.BytesIO()
        records.drain(stream, progress)
        self.assertEqual(records.wait("steady", 0)[0], frames[1])
        self.assertEqual(report["generator_samples"], [frames[2]])
        self.assertEqual(len(progress.getvalue().splitlines()), 3)
        with self.assertRaisesRegex(ScenarioError, "ended before complete"):
            records.wait("complete", 0)

    def test_exit_diagnostics_and_private_config_survive_failure(self):
        # This child only reads stdin and reports what it observed; it opens no sockets.
        code = """import json,sys
cfg=json.load(sys.stdin)
assert cfg["base"] == "http://proxy-b"
print(json.dumps({"phase":"steady"}), flush=True)
print(json.dumps({"phase":"complete","sent":0}), flush=True)
print(cfg["tokens"][0],file=sys.stderr)
sys.exit(7)"""
        report = {}
        cfg = {"base":"http://127.0.0.1:12345", "tokens":["synthetic-private-token"]}
        with tempfile.TemporaryDirectory() as temp, patch.object(load_driver, "commands", return_value=([sys.executable, "-c", code], None)):
            with load_driver.launch(PROJECT, "container", cfg, report, Path(temp)/"progress") as (proc, records):
                self.assertEqual(records.wait("steady", 5)[0]["phase"], "steady")
                self.assertEqual(records.wait("complete", 5)[0]["sent"], 0)
                self.assertEqual(proc.wait(timeout=5), 7)
            self.assertEqual(report["generator_stderr_tail"], "[redacted]")
            self.assertEqual(report["generator_exit_code"], 7)
            self.assertEqual(report["generator_result"]["sent"], 0)
            self.assertNotIn(cfg["tokens"][0], (Path(temp)/"progress").read_text() + json.dumps(report))


if __name__ == "__main__":
    unittest.main()
