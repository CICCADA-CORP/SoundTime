"""Combine authenticated searches, real HTTP audio readers and browser witnesses on a disposable fleet."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack
from email.utils import parsedate_to_datetime
import json
import math
import os
from pathlib import Path
import platform
import re
import resource
import subprocess
import sys
import threading
import time
from urllib.parse import urlsplit
from urllib.error import HTTPError
import load_capacity
import load_driver
import load_public
import load_providers
import load_search

from scenario import (BrowserSession, Instance, ScenarioError, counters, difference, fetch_range,
                      require, wait_for, wav_fixture)

HERE = Path(__file__).resolve().parent
ROLES = ["backend-a", "backend-b", "backend-c", "db-b", "proxy-b"]


def preflight(mode):
    require(mode in ("load-smoke", "load-full"), "Choose a load profile explicitly")
    require(os.getenv("SOUNDTIME_LOAD_CACHE", "cold") in ("cold", "warm"), "Unknown cache profile")
    require(os.getenv("SOUNDTIME_LOAD_FAULT", "none") in ("none", "origin-c"), "Unknown fault profile")
    requested_host = os.getenv("SOUNDTIME_LOAD_HOST")
    require(not requested_host or requested_host == platform.node(), "This is not the selected load host")
    selected = load_driver.location()
    host = {"os": platform.system(), "release": platform.release(), "architecture": platform.machine(),
            "cpus": os.cpu_count(), "generator_location": selected}
    if selected == "container":
        # The launcher checks the actual Linux limits in the built generator image.
        return host
    version = subprocess.check_output(["node", "--version"], text=True).strip()
    require(int(version.lstrip("v").split(".")[0]) >= 22, "Node 22+ is required")
    soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
    wanted = 2048 if mode == "load-full" else 128
    require(hard == resource.RLIM_INFINITY or hard >= wanted,
            f"The generator needs a per-process open-file limit of at least {wanted}")
    if soft != resource.RLIM_INFINITY and soft < wanted:
        resource.setrlimit(resource.RLIMIT_NOFILE, (wanted, hard))
    return {**host, "node": version, "open_file_limit": resource.getrlimit(resource.RLIMIT_NOFILE)[0]}


def compose(project, *args, data=None, timeout=120):
    cmd = ["docker", "compose", "--env-file", "/dev/null", "--project-name", project,
           "--file", str(HERE / "compose.yml")]
    if load_driver.location() == "container":
        cmd += ["--file", str(HERE / "browser.yml")]
    if os.environ.get("SOUNDTIME_TEST_PUBLIC_LOAD") == "1":
        cmd += ["--file",str(HERE / "load-public.yml")]
    cmd.extend(args)
    return subprocess.run(cmd, input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          timeout=timeout, check=True).stdout


def quantiles(values):
    values = sorted(values)
    probs = (("p50", .5), ("p95", .95), ("p99", .99))
    return {"samples": len(values), **{key: round(values[math.ceil(q * len(values)) - 1], 2)
                                     if values else None for key, q in probs}}


def docker_bytes(value):
    units = {"b": 1, "kb": 1000, "mb": 1000**2, "gb": 1000**3,
             "kib": 1024, "mib": 1024**2, "gib": 1024**3, "tib": 1024**4}
    match = re.fullmatch(r"([\d.]+)\s*([a-z]+)", value.strip().lower())
    if not match or match[2] not in units:
        return None
    return round(float(match[1]) * units[match[2]])


def telemetry(project, ids, readers, nodes, stop, samples, errors):
    while not stop.is_set():
        sample = {"unix_s": time.time(), "containers": {}, "cache": {}}
        try:
            rows = subprocess.run(["docker", "stats", "--no-stream", "--format", "{{json .}}", *ids],
                                  capture_output=True, text=True, check=True, timeout=15)
            for line in rows.stdout.splitlines():
                row = json.loads(line)
                role = ids[row["ID"][:12]]
                used = docker_bytes(row["MemUsage"].split("/")[0])
                sample["containers"][role] = {"memory_bytes": used, "cpu": row["CPUPerc"],
                                               "network_total": row["NetIO"]}
            # A paused C must never prevent measurements on the public instance.
            sample["cache"] = nodes[1].call("/admin/storage/status")["cache"]
            sample["proxy_connections"] = load_public.connections(compose, project, readers)
        except Exception as exc:
            errors.append(str(exc))
        samples.append(sample)
        stop.wait(4)


def seed(project, virtual, titles, owner=None):
    output = compose(project, "exec", "-T", "db-b", "psql", "-X", "-qAt", "-U", "soundtime",
                     "-d", "soundtime", "-v", f"origins={virtual}", "-v", f"titles={titles}",
                     "-v", "fenced="+("true" if owner else "false"), "-v", "worker="+(owner or ""),
                     data=(HERE / "load-seed.sql").read_bytes(), timeout=1800)
    return json.loads(output.decode().strip().splitlines()[-1])


def register_listener(node, username, counts):
    deadline = time.monotonic() + 90
    while True:
        try:
            return node.account(username)
        except ScenarioError as exc:
            error = exc.__cause__
            if not isinstance(error, HTTPError) or error.code != 429:
                raise
            require(error.url.endswith("/api/auth/register"), "Unexpected rate limit after account creation")
            value = error.headers.get("Retry-After", "6")
            try:
                pause = max(1, float(value))
            except ValueError:
                pause = max(1, parsedate_to_datetime(value).timestamp() - time.time())
            require(time.monotonic()+pause+.25 < deadline, "Registration Retry-After exceeded preparation budget")
            counts["registration_429"] += 1
            time.sleep(pause+.25)


def mp3_fixture(project, node):
    encoded = compose(project, "exec", "-T", "backend-a", "ffmpeg", "-hide_banner", "-loglevel",
                      "error", "-i", "pipe:0", "-acodec", "libmp3lame", "-b:a", "64k", "-f",
                      "mp3", "pipe:1", data=wav_fixture(16000 * 40, 29))
    return node.upload("Browserwitness", encoded, extension="mp3", mime="audio/mpeg")


def physical_storage(project):
    occupied = {}
    for site in "abc":
        rows = compose(project, "exec", "-T", "backend-" + site, "du", "-sk",
                       "/data/music", "/data/p2p-blobs").decode().splitlines()
        occupied[site.upper()] = {name: int(kib) * 1024 for kib, name in (line.split(None, 1) for line in rows)}
    sql = "SELECT pg_database_size(current_database())"
    occupied["B"]["database"] = int(compose(project, "exec", "-T", "db-b", "psql", "-XqAt",
                                            "-U", "soundtime", "-d", "soundtime", "-c", sql))
    return occupied


def churn(a, b, delay, frame_received, observed):
    time.sleep(delay)
    start = time.monotonic()
    observed["started_ms"] = (start - frame_received) * 1000
    track = a.upload("Loadcatalogchange", wav_fixture(32000, 47))
    observed["origin_track_id"] = track
    observed["reader_track_id"] = wait_for("fresh title at B during load", lambda: b.search("Loadcatalogchange"), timeout=50)
    inserted = time.monotonic()
    observed["insert_observed_ms"] = (inserted - frame_received) * 1000
    observed["visible_insert_ms"] = round((inserted - start) * 1000)
    a.call("/tracks/" + track, method="DELETE")
    wait_for("withdrawal at B during load", lambda: not b.search("Loadcatalogchange"), timeout=50)
    withdrawn = time.monotonic()
    observed["withdraw_observed_ms"] = (withdrawn - frame_received) * 1000
    observed["visible_withdraw_ms"] = round((withdrawn - inserted) * 1000)


def pause_source(project, steady, full):
    time.sleep(28 if full else 3)
    compose(project, "pause", "backend-c")
    began = time.monotonic()
    try:
        time.sleep(12 if full else 3)
    finally:
        compose(project, "unpause", "backend-c")
    return {"source": "C", "start_s_since_steady": round(began - steady, 2),
            "end_s_since_steady": round(time.monotonic() - steady, 2)}


def observe_sound(browser, deadline, report):
    observations = report["sound_browser_samples"]
    while time.monotonic() + 3 < deadline:
        try:
            sample = browser.phase("first_sound")
        except Exception as exc:
            report["sound_browser_error"] = str(exc)
            return
        if time.monotonic() <= deadline:
            observations.append({"unix_s": time.time(), **sample})
        time.sleep(2)


def run(args, report):
    host = preflight(args.mode)
    project = os.getenv("SOUNDTIME_TEST_COMPOSE_PROJECT", "")
    require(re.fullmatch(r"st-fed-[0-9a-f]{16}", project) is not None, "Disposable Compose project required")
    a, b, c = nodes = [Instance(k, getattr(args, k.lower())) for k in "ABC"]
    for node in nodes:
        expected = compose(project, "port", "proxy-" + node.name.lower(), "80").decode().strip()
        url = urlsplit(node.url)
        require(url.scheme == "http" and url.hostname == "127.0.0.1" and url.netloc == expected
                and not url.path, "Only loopback ports of the current disposable project can receive load")

    full = args.mode == "load-full"
    warm = os.getenv("SOUNDTIME_LOAD_CACHE", "cold") == "warm"
    fault = os.getenv("SOUNDTIME_LOAD_FAULT", "none") == "origin-c"
    virtual, titles, media_count = (298, 1000, 12) if full else (4, 20, 3)
    report["profile"] = {"indexed_catalogues": virtual + 2, "virtual_catalogues": virtual,
                         "real_remote_audio_sources": ["A", "C"], "audio_tracks": media_count,
                         "synthetic_tracks_per_catalogue": titles, "target_http_readers": 1000 if full else 6,
                         "target_search_per_second": 100 if full else 3, "hardware": host,
                         "cache": "prewarmed" if warm else "zero disk budget", "pause_c": fault,
                         "network": "local unshaped Docker bridge", "http_reader_codec": "WAV",
                         "browser_witness_codec": "MP3", "browser_clock_poll_interval_ms": 100}
    for node in nodes:
        require(not node.call("/setup/status")["has_admin"], "Disposable empty instances required")
        node.account("admin" + node.name.lower(), admin=True)
        node.identify()
    if full:
        prepared = json.loads(Path(os.environ["SOUNDTIME_TEST_LOAD_INDEX"]).read_text())
        require(prepared["project"] == project, "Index report belongs to a different disposable project")
        generation = int(load_public.sql(compose, project, "SELECT generation FROM federation_worker_lease "
               "WHERE expires_at>clock_timestamp()"))
        require(generation > prepared["generation"], "Public worker did not acquire a new generation")
        report["index_preparation"] = prepared
        report["serving_worker_generation"] = generation
        report["seed"] = prepared["seed"]
    report["journal_preparation"] = {}
    load_public.wait_for_journals(compose, project, report["journal_preparation"], 900 if full else 120)
    report["search_index_preparation"] = {}
    load_search.wait_for_index(compose, project, report["search_index_preparation"], 900 if full else 120)
    for node in nodes:
        refreshed = node.call("/auth/refresh", method="POST", token="",
                              body={"refresh_token": node.refresh_token})
        node.token, node.refresh_token = refreshed["access_token"], refreshed["refresh_token"]
    for source in (a, c):
        b.connect(source, follow=True)
        source.connect(b, follow=True)

    media = []
    for index in range(media_count):
        owner = (a, c)[index % 2]
        value, title = 3 + 4 * index, "Loadmusic" + chr(ord("a") + index)
        audio = wav_fixture((8 if full else 2) * 1024**2, value)
        owner.upload(title, audio)
        track = wait_for(f"{owner.name} audio on B", lambda: b.search(title))
        media.append({"id": track, "size": len(audio), "seed": value, "source": owner.name})
        if warm:
            fetch_range(b, track, audio, 0, len(audio))
    mp3_fixture(project, a)
    witness = wait_for("browser MP3 on B", lambda: b.search("Browserwitness"))
    if not full:
        report["seed"] = seed(project, virtual, titles)
    require(report["seed"] == {"virtual_catalogues": virtual, "virtual_tracks": virtual * titles,
                               "indexed_tracks": virtual * titles}, "Incomplete synthetic index")
    wait_for("metadata searchable after FTS reconciliation",
             lambda: b.call("/search?q=Quartz0001&include_p2p=false")["tracks"], timeout=120)
    if full:
        report["local_search_plans"] = load_search.explain(compose, project, args.report)
    report["range_provider_resolution"] = load_providers.explain(compose, project, media[0]["id"], args.report)
    cache = report["cache_before"] = b.call("/admin/storage/status")["cache"]
    if warm:
        require(cache["evictable_bytes"] >= sum(row["size"] for row in media), "Prewarming did not admit the audio")
    else:
        require(cache["maximum_bytes"] == 0, "Cold profile requires zero-disk admission")

    # Create regular listeners after the index build to keep their JWTs fresh.
    report["auth_preparation"] = auth = {"registration_429": 0}
    accounts = [register_listener(b, "loadlistener"+str(i), auth) for i in range(32 if full else 2)]
    pid = b.playlist(accounts[0], "Load witness", [witness])
    report["profile"]["distinct_listener_accounts"] = len(accounts)
    report["physical_bytes_before"] = physical_storage(project)
    roles = ROLES + (["load-generator"] if host["generator_location"] == "container" else [])
    ids, readers = load_public.containers(compose, project, roles, full)
    report["nginx_and_connections"] = load_public.nginx_settings(compose, project)
    if full:
        require(report["nginx_and_connections"]["catalogue_least_conn_enabled"],"Public read tier missing from Nginx")
        stream_limits = compose(project, "exec", "-T", "backend-b", "printenv",
                                "SOUNDTIME_MAX_ACTIVE_STREAMS", "SOUNDTIME_MAX_STREAMS_PER_ACCOUNT",
                                "SOUNDTIME_MAX_ANONYMOUS_STREAMS").decode().splitlines()
        report["profile"]["http_stream_limits"] = dict(zip(
            ("instance", "per_account", "anonymous"), map(int, stream_limits), strict=True))
        minimum, maximum = compose(project,"exec","-T","catalog-http","printenv",
                                   "DB_MIN_CONNECTIONS","DB_MAX_CONNECTIONS").decode().splitlines()
        report["nginx_and_connections"]["catalogue_pool_per_reader"] = {"minimum":int(minimum),"maximum":int(maximum)}
    report["profile"]["catalogue_http_readers"] = len(readers)
    baseline = counters(nodes)
    report["container_samples"], report["monitor_errors"], report["browser_samples"] = [], [], []
    samples, errors, browsers = report["container_samples"], report["monitor_errors"], report["browser_samples"]
    report["sound_browser_samples"] = []
    stop = threading.Event()
    with ExitStack() as stack:
        browser = BrowserSession(stack, b, witness, "Browserwitness", [pid], [accounts[0]])
        auth["browser"] = browser.phase("authenticate")
        if full:
            sound_playlist = b.playlist(accounts[1], "Playback witness", [witness])
            sound_browser = BrowserSession(stack, b, witness, "Browserwitness", [sound_playlist], [accounts[1]])
            auth["sound_browser"] = sound_browser.phase("authenticate")
        routed_before = load_public.routed_searches(compose,project,readers) if full else {}
        thread = threading.Thread(target=telemetry, args=(project, ids, readers, nodes, stop, samples, errors), daemon=True)
        thread.start()

        def finish_telemetry():
            stop.set()
            thread.join(60)
        stack.callback(finish_telemetry)
        cfg = {"base": b.url, "tokens": [account[1] for account in accounts], "profile": args.mode,
               "virtual_catalogues": virtual, "media": media}
        with load_driver.launch(project, host["generator_location"], cfg, report,
                                args.report.with_name("generator-progress.jsonl")) as (generator, records):
            announced, received = records.wait("steady", timeout=80 if full else 50)
            seconds = 90 if full else 15
            # The container and the orchestrator can have different wall clocks.
            # Anchor the remaining plateau on receipt of the generator's frame.
            deadline = received + min(seconds, max(0, announced["remaining_ms"] / 1000))
            steady = deadline - seconds
            # The generator starts its ramp after reading our config, and emits
            # the steady frame before we receive it. These two causal bounds
            # do not require synchronised clocks or negligible Docker pipe lag.
            earliest_end = records.config_sent_at + announced["ramp_ms"] / 1000 + seconds
            report["catalogue_observation_window_ms"] = {
                "earliest_end_ms": (earliest_end - received) * 1000,
                "latest_end_ms": (deadline - received) * 1000,
            }
            observed = report["live_catalogue_change"] = {}
            with ThreadPoolExecutor(max_workers=3) as executor:
                sound = executor.submit(observe_sound, sound_browser, deadline, report) if full else None
                mutation = executor.submit(churn, a, b, 10 if full else 2, received, observed)
                failure = executor.submit(pause_source, project, steady, full) if fault else None
                while time.monotonic() + 3 < deadline and generator.poll() is None:
                    try:
                        obs = browser.phase("measure")
                        if time.monotonic() <= deadline:
                            browsers.append({"unix_s": time.time(), "search_to_screen_ms": obs["search_to_screen_ms"],
                                             "navigation_timing": obs.get("navigation_timing"),
                                             "first_media_progress_ms": obs["first_progress_ms"],
                                             "seek_progress": obs["after_seek_seconds"]})
                    except Exception as exc:
                        report["browser_error"] = str(exc)
                        break
                records.wait("complete", timeout=max(5, deadline+30-time.monotonic()))
                require(generator.wait(timeout=15) == 0, "Generator failed after writing its result")
                try:
                    mutation.result(timeout=120)
                except Exception as exc:
                    report["live_catalogue_change_error"] = str(exc)
                if sound:
                    try:
                        sound.result(timeout=125)
                    except Exception as exc:
                        report["sound_browser_error"] = str(exc)
                if failure:
                    try:
                        report["fault"] = failure.result(timeout=60)
                    except Exception as exc:
                        report["fault_error"] = str(exc)
    # A saturated worker can fail ancillary requests. Retain the independent
    # generator and browser observations and identify missing server evidence.
    for field, probe in (
        ("transport_delta", lambda: difference(baseline, counters(nodes))),
        ("cache_after", lambda: b.call("/admin/storage/status")["cache"]),
        ("physical_bytes_after", lambda: physical_storage(project)),
    ):
        try:
            report[field] = probe()
        except Exception as exc:
            errors.append(f"{field}: {exc}")
    routed = report["successful_searches_per_reader"] = {}
    if full:
        try:
            after = load_public.routed_searches(compose, project, readers)
            routed.update({key: end-routed_before[key] for key,end in after.items()})
        except Exception as exc:
            errors.append(f"routed searches: {exc}")
    report["first_sound_ms"] = quantiles([s["first_sound_ms"] for s in report["sound_browser_samples"]])
    for key in ("search_to_screen_ms", "first_media_progress_ms"):
        report[key] = quantiles([row[key] for row in browsers])
    report["container_peak_memory_bytes"] = {
        role: max((row["containers"].get(role, {}).get("memory_bytes") or 0 for row in samples), default=0)
        for role in ids.values()}
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    report["orchestrator_peak_rss_bytes"] = peak if sys.platform == "darwin" else peak * 1024
    assessment = report["capacity"] = load_capacity.assess(report)
    report["workload_reached"] = assessment["workload_reached"]
    report["valid_browser_measurements"] = all(ok for name, ok in assessment["evidence"].items()
                                               if name.startswith("browser_"))
    report["screen_p95_within_300ms_in_sample"] = assessment["service"]["screen_p95_within_300ms"]
    report["status"] = "measured"
    if fault and assessment["verdict"] == "fault_observation_incomplete":
        raise ScenarioError("Fault injection or witness evidence incomplete; inspect report.json")
    if full and not fault and assessment["verdict"] != "passed":
        raise ScenarioError(f"Public capacity {assessment['verdict']}; inspect capacity and observations in report.json")
    if not full and not fault and (not report["workload_reached"] or not report["valid_browser_measurements"]):
        raise ScenarioError("Smoke workload or browser observations incomplete; inspect report.json")


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--preflight":
        preflight(sys.argv[2])
        return
    if len(sys.argv) == 3 and sys.argv[1] == "--prepare-public":
        load_public.prepare(compose,seed,sys.argv[2])
        return
    parser = argparse.ArgumentParser()
    for name in ("a", "b", "c"):
        parser.add_argument("--" + name, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--mode", choices=["load-smoke", "load-full"], required=True)
    args = parser.parse_args()
    report = {"status": "failed", "mode": args.mode, "utc_start": time.time()}
    try:
        run(args, report)
    except Exception as exc:
        report["error"] = str(exc)
        raise
    finally:
        report["utc_end"] = time.time()
        args.report.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
