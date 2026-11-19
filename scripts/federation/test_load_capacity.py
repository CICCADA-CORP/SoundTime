"""Synthetic observations, no HTTP, Docker or PostgreSQL."""
import unittest

from load_capacity import assess


def observation():
    rows = [{"listener": i, "bytes": 32768 * 90, "bytes_per_second": 32768, "longest_unbuffered_ms": 0}
            for i in range(1000)]
    return {
        "mode": "load-full", "profile": {"target_http_readers": 1000, "target_search_per_second": 100,
                                        "cache": "zero disk budget", "pause_c": False},
        "generator_result": {"load": {"target_bytes_per_second_per_reader": 32768, "steady_seconds": 90,
                                      "steady_samples": 89, "samples_at_target": 89, "max_sample_gap_ms": 1010,
                                      "samples_with_all_reader_sessions": 89},
                             "search_due": 9000, "search_started": 9000, "search_completed": 8990,
                             "search_failed": 0, "search_open_at_deadline": 10, "omissions": 0,
                             "media_attempts": 1000, "media_starts": 1000, "media_failed_starts": 0,
                             "interruptions": 0, "useful_bytes": 32768 * 90 * 1000, "per_reader_delivery": rows},
        "transport_delta": {site: {"sent": 1} for site in "ABC"},
        "successful_searches_per_reader": {"catalog-http-1": 4400, "catalog-http-2": 4590},
        "container_peak_memory_bytes": {"catalog-http-1": 1, "catalog-http-2": 1},
        "live_catalogue_change": {}, "search_to_screen_ms": {"samples": 20, "p95": 300},
        "first_media_progress_ms": {"samples": 20, "p95": 800}, "first_sound_ms": {"samples": 21, "p95": 450},
    }


class Capacity(unittest.TestCase):
    def test_completed_throughput_boundary_and_explicit_errors(self):
        report = observation()
        result = report["generator_result"]
        self.assertEqual(assess(report)["verdict"], "passed")
        result.update(search_completed=8910, search_open_at_deadline=90)
        self.assertEqual(assess(report)["verdict"], "passed")
        result.update(search_completed=8909, search_open_at_deadline=91)
        self.assertEqual(assess(report)["verdict"], "not_met")
        result.update(search_completed=8910, search_failed=1, search_open_at_deadline=89)
        self.assertEqual(assess(report)["verdict"], "not_met")

    def test_generator_diagnostics_are_descriptive_and_support_older_reports(self):
        report = observation()
        original = assess(report)
        self.assertEqual(original["generator_observation"]["steady_timing"], "unavailable")
        self.assertIsNone(original["generator_observation"]["omissions_reconcile"])
        result = report["generator_result"]
        result["generator"] = {
            "pending_search_limit": 300, "search_dispatch_lag_ms": {"p95": 18, "samples": 9000},
            "steady_window": {"observed_ms": 89996, "start_lag_ms": 5, "end_lag_ms": 1,
                              "event_loop_delay_samples": 5800, "process_cpu_cores": 1.12,
                              "event_loop_utilization": .98, "event_loop_delay_p99_ms": 29},
        }
        reasons = dict(inflight_limit=0, dispatch_late=0, inflight_limit_and_dispatch_late=0, window_ended=0)
        result["omissions_by_reason"] = reasons
        observed = assess(report)
        witness = observed["generator_observation"]
        self.assertEqual(observed["verdict"], original["verdict"])
        self.assertEqual(witness["steady_timing"], "observed")
        self.assertEqual(witness["steady_process_cpu_cores"], 1.12)
        self.assertTrue(witness["dispatch_p95_exceeded_scheduled_interval"])
        self.assertTrue(witness["omissions_reconcile"])
        reasons.update(inflight_limit=1, dispatch_late=2, inflight_limit_and_dispatch_late=3, window_ended=4)
        result.update(omissions=10, search_started=8990, search_open_at_deadline=0)
        reduced = assess(report)
        self.assertEqual(reduced["verdict"], "offered_load_incomplete")
        self.assertEqual(reduced["generator_observation"]["omissions_by_reason"], reasons)
        self.assertTrue(reduced["generator_observation"]["omissions_reconcile"])
        result["generator"]["steady_window"]["start_lag_ms"] = 4000
        partial = assess(report)["generator_observation"]
        self.assertEqual(partial["steady_timing"], "partial")
        self.assertIsNone(partial["steady_event_loop_delay_p99_ms"])
        self.assertIsNone(partial["steady_process_cpu_cores"])

    def test_missing_proposed_load(self):
        report = observation()
        report["generator_result"].update(search_started=8999, omissions=1, search_open_at_deadline=9)
        self.assertEqual(assess(report)["verdict"], "offered_load_incomplete")

    def test_global_bytes_and_open_sockets_can_hide_one_starved_listener(self):
        report = observation()
        rows = report["generator_result"]["per_reader_delivery"]
        rows[0].update(bytes=0, bytes_per_second=0, longest_unbuffered_ms=90000)
        rows[1].update(bytes=2*32768*90, bytes_per_second=65536)
        result = assess(report)
        self.assertEqual(result["verdict"], "not_met")
        self.assertEqual(result["per_reader_delivery"]["below_rate"], [0])
        self.assertEqual(result["per_reader_delivery"]["unbuffered_over_limit"], [0])
        self.assertTrue(result["evidence"]["all_listeners_accounted"])

    def test_browser_and_sampling_evidence(self):
        report = observation()
        report["search_to_screen_ms"]["samples"] = 1
        self.assertEqual(assess(report)["verdict"], "insufficient_evidence")
        report["search_to_screen_ms"].update(samples=20, p95=301)
        self.assertEqual(assess(report)["verdict"], "not_met")
        report["generator_result"]["load"]["max_sample_gap_ms"] = 5000
        self.assertFalse(assess(report)["workload_reached"])

    def test_fault_replay_does_not_qualify_capacity(self):
        report = observation()
        report["profile"]["pause_c"] = True
        self.assertEqual(assess(report)["verdict"], "fault_observation_incomplete")
        report["fault"] = {"source": "C"}
        report["generator_result"]["interruptions"] = 20
        self.assertEqual(assess(report)["verdict"], "fault_observation")
        self.assertFalse(assess(report)["service"]["zero_media_interruptions"])


if __name__ == "__main__":
    unittest.main()
