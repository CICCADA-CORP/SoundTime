"""Assess a steady-state observation independently from generator targets."""
import math


def describe_generator(result, rate, seconds, steady_observed):
    """Describe what the client measured independently of the capacity verdict."""
    generator = result.get("generator", {})
    window = generator.get("steady_window") or {}
    reasons = result.get("omissions_by_reason")
    dispatch = generator.get("search_dispatch_lag_ms", {})
    interval = 1000 / rate
    p95 = dispatch.get("p95")
    # Use the same coverage and maximum observation gap as the plateau assessment.
    coverage = (steady_observed and window.get("observed_ms", 0) >= .90 * seconds * 1000
                and window.get("start_lag_ms", math.inf) <= 2500
                and window.get("end_lag_ms", math.inf) <= 2500
                and window.get("event_loop_delay_samples", 0) > 0)
    return {
        "steady_timing": "observed" if coverage else ("partial" if window else "unavailable"),
        "steady_process_cpu_cores": window.get("process_cpu_cores") if coverage else None,
        "steady_event_loop_utilization": window.get("event_loop_utilization") if coverage else None,
        "steady_event_loop_delay_p99_ms": window.get("event_loop_delay_p99_ms") if coverage else None,
        "scheduled_interval_ms": interval, "dispatch_lag_samples": dispatch.get("samples", 0),
        "dispatch_lag_p95_ms": p95,
        "dispatch_p95_exceeded_scheduled_interval": p95 > interval if p95 is not None else None,
        "pending_search_limit": generator.get("pending_search_limit"),
        "omissions_by_reason": reasons,
        "omissions_reconcile": sum(reasons.values()) == result["omissions"] if reasons else None,
    }


def assess(report):
    full = report["mode"] == "load-full"
    profile = report["profile"]
    result = report["generator_result"]
    load = result["load"]
    clients = profile["target_http_readers"]
    seconds = 90 if full else 15
    expected = seconds * profile["target_search_per_second"]
    audio_fraction = .95 if full else .80
    audio_target = load["target_bytes_per_second_per_reader"]
    rows = result["per_reader_delivery"]
    delivery = sorted(row["bytes_per_second"] for row in rows)
    audio_below = [r["listener"] for r in rows if r["bytes_per_second"] < audio_fraction * audio_target]
    stalled = [r["listener"] for r in rows if r["longest_unbuffered_ms"] > 2000]
    offered = {
        "all_searches_scheduled": result["search_due"] == expected and result["search_started"] == expected
                                  and result["omissions"] == 0,
        "all_reader_sessions_started": load["steady_samples"] > 0
                                      and load["samples_with_all_reader_sessions"] == load["steady_samples"],
    }
    achieved = {
        "all_responses_open_at_each_sample": load["steady_samples"] > 0
                                            and load["samples_at_target"] == load["steady_samples"],
        "successful_searches_within_window": result["search_completed"] >= math.ceil(expected * .99),
        "every_listener_received_required_bytes": len(rows) == clients and not audio_below,
        "both_origins_served": profile["cache"] == "prewarmed" or all(
            report.get("transport_delta", {}).get(origin, {}).get("sent", 0) > 0 for origin in ("A", "C")),
    }
    minimum_witness = 20 if full else 1
    screen = report["search_to_screen_ms"]
    progress = report["first_media_progress_ms"]
    sound = report["first_sound_ms"]
    routed = report["successful_searches_per_reader"]
    reader_roles = ["catalog-http-1", "catalog-http-2"] if full else []
    evidence = {
        "steady_observation": load["steady_seconds"] == seconds
                            and load["steady_samples"] >= math.ceil(.90 * (seconds - 1))
                            and load["max_sample_gap_ms"] <= 2500,
        "search_accounting": result["search_due"] == result["search_started"] + result["omissions"]
                            and result["search_started"] == (result["search_completed"] + result["search_failed"]
                                                             + result["search_open_at_deadline"]),
        "all_listeners_accounted": {r["listener"] for r in rows} == set(range(clients))
                                  and len(rows) == clients and sum(r["bytes"] for r in rows) == result["useful_bytes"],
        "transport_observed": all(origin in report.get("transport_delta", {}) for origin in ("A", "B", "C")),
        "reader_tier_observed": all(routed.get(r, 0) > 0 and report["container_peak_memory_bytes"].get(r, 0) > 0
                                    for r in reader_roles),
        "browser_screen_witness": screen["samples"] >= minimum_witness and "browser_error" not in report,
        "browser_playback_witness": progress["samples"] >= minimum_witness and "browser_error" not in report,
        "browser_first_sound_witness": not full or (sound["samples"] >= minimum_witness and "sound_browser_error" not in report),
    }
    service = {
        "catalogue_mutation_completed": "live_catalogue_change" in report,
        "zero_search_failures": result["search_failed"] == 0,
        "zero_failed_media_starts": result["media_failed_starts"] == 0,
        "zero_media_interruptions": result["interruptions"] == 0,
        "no_listener_unbuffered_over_2s": not stalled,
        "screen_p95_within_300ms": screen["p95"] is not None and screen["p95"] <= 300,
    }
    reached = (all(offered.values()) and all(achieved.values()) and evidence["steady_observation"]
               and evidence["all_listeners_accounted"] and evidence["search_accounting"])
    if profile["pause_c"]:
        fault_observed = ("fault" in report and evidence["reader_tier_observed"]
                          and screen["samples"] > 0 and (not full or sound["samples"] > 0))
        verdict = "fault_observation" if fault_observed else "fault_observation_incomplete"
    elif not full:
        verdict = "smoke_observation"
    elif not all(evidence.values()):
        verdict = "insufficient_evidence"
    elif not all(offered.values()):
        verdict = "offered_load_incomplete"
    elif not reached or not all(service.values()):
        verdict = "not_met"
    else:
        verdict = "passed"
    return {
        "targets": {"readers": clients, "scheduled_searches": expected, "steady_seconds": seconds,
                    "minimum_completed_searches": math.ceil(.99 * expected), "minimum_browser_samples": minimum_witness,
                    "audio_target_bytes_per_second_per_reader": audio_target,
                    "minimum_audio_bytes_per_second_per_reader": audio_target * audio_fraction,
                    "maximum_unbuffered_ms": 2000, "screen_p95_target_ms": 300},
        "offered": offered, "achieved": achieved, "evidence": evidence, "service": service,
        "workload_reached": reached, "verdict": verdict,
        "generator_observation": describe_generator(result, profile["target_search_per_second"], seconds,
                                                   evidence["steady_observation"]),
        "delivered_searches_per_second": result["search_completed"] / seconds,
        "search_failure_fraction": result["search_failed"] / max(1, result["search_started"]),
        "open_at_deadline": result["search_open_at_deadline"],
        "media_failed_start_fraction": result["media_failed_starts"] / max(1, result["media_attempts"]),
        "media_interruptions_per_start": result["interruptions"] / max(1, result["media_starts"]),
        "per_reader_delivery": {"minimum_bytes_per_second": delivery[0] if delivery else None,
                                "below_rate": audio_below, "unbuffered_over_limit": stalled},
    }
