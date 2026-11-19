import io
import json
import subprocess
import sys
import tempfile
import unittest
from contextlib import ExitStack
from pathlib import Path
from unittest.mock import Mock, call, patch

import scenario
import historical
import load
import load_public
import retention


class Response(io.BytesIO):
    def __init__(self, body, status=206, content_range="bytes 1-3/5"):
        super().__init__(body)
        self.status = status
        self.headers = {"Content-Range": content_range}


class FederationScenarioTests(unittest.TestCase):
    def open_fake_browser(self, stack, proc):
        node = scenario.Instance("B", "http://unused")
        node.login_credentials["user"] = {"username": "reader", "password": "fixture"}
        stack.enter_context(patch.dict(scenario.os.environ, {"FEDERATION_BROWSER": "native"}))
        stack.enter_context(patch.object(scenario.subprocess, "Popen", return_value=proc))
        return scenario.BrowserSession(stack, node, "track", "title", ["playlist"], [("user", "token")])

    def test_closed_browser_input_keeps_the_diagnostic_and_reaps_once(self):
        proc = Mock()
        proc.stdin.flush.side_effect = BrokenPipeError()
        proc.stdin.close.side_effect = BrokenPipeError()
        proc.wait.side_effect = [subprocess.TimeoutExpired("browser", 5),
                                 subprocess.TimeoutExpired("browser", 3), -9]
        with self.assertRaisesRegex(scenario.ScenarioError, "measure.*input pipe closed.*forward lost"):
            with ExitStack() as stack:
                browser = self.open_fake_browser(stack, proc)
                browser.errors.write("forward lost")
                browser.errors.flush()
                browser.phase("measure")
        browser.close()
        self.assertEqual(proc.wait.call_args_list, [call(timeout=5), call(timeout=3), call(timeout=2)])
        proc.terminate.assert_called_once()
        proc.kill.assert_called_once()
        proc.stdin.close.assert_called_once()
        proc.stdout.close.assert_called_once()

    def test_load_cli_passes_report_path_for_sibling_progress(self):
        def write_progress(args, report):
            self.assertIsInstance(args.report, Path)
            progress = args.report.with_name("generator-progress.jsonl")
            progress.write_text('{"phase":"complete"}\n')
            report["progress_file"] = progress.name

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "load report.json"
            argv = ["load.py", "--a", "A", "--b", "B", "--c", "C", "--mode", "load-smoke",
                    "--report", str(path)]
            with patch.object(load.sys, "argv", argv), patch.object(load, "run", side_effect=write_progress):
                load.main()
            self.assertEqual(json.loads(path.read_text())["progress_file"], "generator-progress.jsonl")
            self.assertEqual(json.loads((path.parent / "generator-progress.jsonl").read_text()),
                             {"phase": "complete"})

    def test_load_error_retains_diagnostics_and_partial_results_when_witness_wont_exit(self):
        proc = Mock()
        proc.stdin.close.side_effect = BrokenPipeError()
        proc.wait.side_effect = subprocess.TimeoutExpired("browser", 2)
        samples = [{"unix_s": 123, "search_to_screen_ms": 122}]
        result = {"phase": "complete", "search_failed": 2}

        def lose_forward(_args, report):
            with ExitStack() as stack:
                self.open_fake_browser(stack, proc)
                report.update(generator_result=result, browser_samples=samples)
                raise ConnectionRefusedError("public service port forward closed")

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "report.json"
            args = ["load.py", "--a", "A", "--b", "B", "--c", "C", "--mode", "load-full",
                    "--report", str(path)]
            with patch.object(load.sys, "argv", args), patch.object(load, "run", side_effect=lose_forward), \
                 self.assertRaisesRegex(ConnectionRefusedError, "public service port forward closed"):
                load.main()
            saved = json.loads(path.read_text())
            self.assertEqual(saved["error"], "public service port forward closed")
            self.assertEqual(saved["generator_result"], result)
            self.assertEqual(saved["browser_samples"], samples)
        proc.kill.assert_called_once()

    def test_archive_retries_only_initial_missing_address(self):
        peer = scenario.Instance("B", "")
        peer.node_id = "peer-b"
        origin = scenario.Instance("C", "")
        no_route = "ping failed: connection error: No addressing information available"
        for replies, count in [
                ([no_route, "pong received: Pong { node_id: peer-b }"], 2),
                (["ping failed: connection error: timed out"], 1),
                (["pong received: Pong { node_id: someone-else }"], 1)]:
            with self.subTest(replies=replies), patch.object(scenario.time, "sleep"), \
                 patch.object(origin, "call", side_effect=[{"message": s} for s in replies]) as dial:
                if len(replies) == 2:
                    self.assertEqual(historical.initial_archived_ping(origin, peer), count)
                else:
                    with self.assertRaisesRegex(scenario.ScenarioError, "unexpected historical Pong"):
                        historical.initial_archived_ping(origin, peer)
                self.assertEqual(dial.call_count, count)

    def test_midstream_outage_checks_the_same_http_body_after_the_cut(self):
        window = 65536
        audio = scenario.wav_fixture(window * 3, 19)
        size = len(audio)
        nodes = [scenario.Instance(name, "http://unused") for name in "ABC"]
        before = {name: {"sent":0, "received":0} for name in "ABC"}
        cut = {"A":{"sent":window,"received":0},"B":{"sent":0,"received":0},
               "C":{"sent":0,"received":window}}
        end = {"B":{"sent":size-window,"received":0},"C":{"sent":0,"received":size}}
        for body in (audio, audio[:-1]+bytes([audio[-1]^1])):
            with self.subTest(correct=body==audio):
                response = Response(body, content_range=f"bytes 0-{size-1}/{size}")
                response.headers["Content-Length"] = str(size)
                observation = {}
                with patch.object(nodes[2], "open", return_value=response) as opened, \
                     patch.object(retention, "counters", side_effect=[before,cut,end]), \
                     patch.object(retention, "switch_backend") as stopped:
                    if body == audio:
                        retention.lose_origin_during_response(*nodes,"track",audio,observation)
                        self.assertEqual(observation["verified_bytes"], size)
                    else:
                        with self.assertRaisesRegex(scenario.ScenarioError, "incorrect continuous audio"):
                            retention.lose_origin_during_response(*nodes,"track",audio,observation)
                    stopped.assert_called_once_with("A", False)
                    opened.assert_called_once_with("/tracks/track/stream", headers={"Range":"bytes=0-"})

    def test_public_load_waits_for_journal_after_projection_completed(self):
        ready = json.dumps({"projection_complete":True, "journal_complete":True}).encode()
        pending = json.dumps({"projection_complete":True, "journal_complete":False}).encode()
        compose = Mock(side_effect=[ready,pending,ready,ready])
        observed = {}
        with patch.object(load_public.time, "sleep") as pause:
            load_public.wait_for_journals(compose, "fixture", observed, 30)
        self.assertEqual(observed["polls"], 2)
        self.assertEqual([call.args[3] for call in compose.call_args_list], ["db-a","db-b","db-c","db-b"])
        self.assertIn("federation_event_bootstrap",compose.call_args_list[0].args[-1])
        pause.assert_called_once()

    def test_public_limits_count_each_pool_and_the_effective_shm_mount(self):
        def compose(_project, *args):
            if "nginx" in args:
                return b"worker_connections 4096;\nworker_rlimit_nofile 8192;"
            if "ps" in args:
                return b"17 nginx: worker process\n18 nginx: worker process"
            if "awk" in args:
                return b"8192 8192\n8192 8192"
            if "printenv" in args:
                return b"2\n24" if "backend-b" in args else b"1\n8"
            self.assertIn("df", args)
            return b"Filesystem Blocks Used Available Capacity Mounted on\nshm 262144 1 262143 1% /dev/shm"

        pg = json.dumps({"reserved_slots": 3, "parallel_workers_per_gather": 1, "lease_sessions": 1})
        with patch.dict(scenario.os.environ, {"SOUNDTIME_TEST_PUBLIC_LOAD": "1"}), \
             patch.object(load_public, "sql", side_effect=["64", pg, "44", pg]):
            observed = load_public.nginx_settings(compose, "project")["database_budget"]
            self.assertEqual(observed["application_pool_ceiling"], 41)
            self.assertEqual(observed["remaining_unreserved_slots"], 20)
            self.assertEqual(observed["shared_memory_mount_bytes"], 256*1024*1024)
            with self.assertRaisesRegex(scenario.ScenarioError, "exhaust PostgreSQL"):
                load_public.nginx_settings(compose, "project")

    def test_saturated_postgres_preserves_the_proxy_socket_measurement(self):
        tcp = b"1: 0A000001:0050 0A000002:FFFF 01 0:0\n2: 0A000001:CCCC 0A000003:1F90 01 0:0"
        with patch.object(load_public, "sql", side_effect=subprocess.CalledProcessError(2, "psql")):
            sample = load_public.connections(Mock(return_value=tcp), "test", {"10.0.0.3:8080": "reader"})
        self.assertEqual((sample["inbound_established"], sample["upstream_established"]), (1, 1))
        self.assertIn("database_sampling_error", sample)

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
