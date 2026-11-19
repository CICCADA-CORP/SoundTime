import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import scenario


class Response(io.BytesIO):
    def __init__(self, body, status=206, content_range="bytes 1-3/5"):
        super().__init__(body)
        self.status = status
        self.headers = {"Content-Range": content_range}


class FederationScenarioTests(unittest.TestCase):
    def test_fixture_audio_is_reproducible_and_decodable(self):
        data = scenario.wav_fixture(10001, 7)
        self.assertEqual(scenario.wav_fixture(10001, 7), data)
        self.assertNotEqual(scenario.wav_fixture(10001, 11), data)
        with scenario.wave.open(io.BytesIO(data), "rb") as wav:
            self.assertEqual(wav.getnframes(), 10001)
            self.assertEqual(wav.getframerate(), 16000)
            self.assertGreater(len(set(wav.readframes(10001))), 1)

    def test_remote_range_requires_correct_bytes_and_offsets(self):
        for response in (Response(b"bcd", status=200), Response(b"bad"),
                         Response(b"bcd", content_range="bytes 0-2/5")):
            with self.subTest(headers=response.headers, status=response.status):
                node = Mock(name="B")
                node.open.return_value = response
                with self.assertRaises(scenario.ScenarioError):
                    scenario.fetch_range(node, "id", b"abcde", 1, 3)
        node = Mock()
        node.open.return_value = Response(b"bcd")
        self.assertEqual(scenario.fetch_range(node, "id", b"abcde", 1, 3), 3)
        node.open.assert_called_once_with("/tracks/id/stream", headers={"Range": "bytes=1-3"})

    def test_counter_reset_is_a_failed_measurement(self):
        sample = {"B": {"sent": 10, "received": 3}}
        next_sample = {"B": {"sent": 48, "received": 3}}
        self.assertEqual(scenario.difference(sample, next_sample), {"B": {"sent": 38, "received": 0}})
        with self.assertRaises(scenario.ScenarioError):
            scenario.difference(next_sample, sample)

    def test_presence_is_checked_before_metadata_filtering(self):
        node = Mock()
        node.name = "A"
        node.telemetry.return_value = {"catalog_entries_received": 5, "audio_bytes_sent": 0,
                                       "audio_bytes_received": 0, "range_proof_bytes_received": 0,
                                       "range_windows_verified": 0, "relay_windows_verified": 0}
        start = scenario.counters([node])
        node.telemetry.return_value["catalog_entries_received"] += 1
        with patch.object(scenario.time, "sleep"), self.assertRaises(scenario.ScenarioError):
            scenario.quiet_catalogue([node], start, {}, "presence")
        self.assertTrue(scenario.no_audio(scenario.difference(start, scenario.counters([node]))))

    def test_legacy_manual_add_preserves_omitted_follow(self):
        local = scenario.Instance("A", "http://unused")
        peer = scenario.Instance("B", "http://unused")
        peer.node_id, peer.addresses = "B", ["127.0.0.1:1204"]
        with patch.object(local, "call") as call:
            local.add_peer(peer)
            self.assertNotIn("follow_catalog", call.call_args.kwargs["body"])
            local.add_peer(peer, follow=False)
            self.assertIs(call.call_args.kwargs["body"]["follow_catalog"], False)

    def test_container_browser_uses_only_the_disposable_ingress_proxy(self):
        node = scenario.Instance("B", "http://127.0.0.1:32049")
        with patch.dict(scenario.os.environ, {"FEDERATION_BROWSER": "container",
                       "SOUNDTIME_TEST_COMPOSE_PROJECT": "st-fed-0123456789abcdef"}):
            command = scenario.browser_command(node, "playlist", "track", "title")
            self.assertIn("http://proxy-b", command)
            self.assertNotIn(node.url, command)
            self.assertIn("--no-deps", command)
            self.assertIn("-T", command)
        with patch.dict(scenario.os.environ, {"FEDERATION_BROWSER": "container",
                       "SOUNDTIME_TEST_COMPOSE_PROJECT": "production"}), self.assertRaises(scenario.ScenarioError):
            scenario.browser_command(node, "playlist", "track", "title")
        with patch.dict(scenario.os.environ, {"FEDERATION_BROWSER": "native"}):
            self.assertIn(node.url, scenario.browser_command(node, "playlist", "track", "title"))

    def test_distinct_instances_are_required_in_both_modes(self):
        with tempfile.TemporaryDirectory() as directory:
            for mode in ("acceptance", "baseline"):
                dest = Path(directory) / mode / "report.json"
                command = [sys.executable, scenario.__file__, "--mode", mode, "--report", str(dest)]
                for node in ("a", "b", "c"):
                    command.extend(["--" + node, "http://localhost:12345"])
                ran = subprocess.run(command, capture_output=True, timeout=10)
                self.assertNotEqual(ran.returncode, 0)
                result = json.loads(dest.read_text())
                self.assertEqual(result["status"], "failed")
                self.assertIn("distinct", result["error"])


if __name__ == "__main__":
    unittest.main()
