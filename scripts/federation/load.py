"""Combine authenticated searches, real HTTP audio readers and browser witnesses on a disposable fleet."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack
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
    version = subprocess.check_output(["node", "--version"], text=True).strip()
    require(int(version.lstrip("v").split(".")[0]) >= 22, "Node 22+ is required")
    soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
    wanted = 2048 if mode == "load-full" else 128
    require(hard == resource.RLIM_INFINITY or hard >= wanted,
            f"The generator needs a per-process open-file limit of at least {wanted}")
    if soft != resource.RLIM_INFINITY and soft < wanted:
        resource.setrlimit(resource.RLIMIT_NOFILE, (wanted, hard))
    return {"node": version, "os": platform.system(), "release": platform.release(),
            "architecture": platform.machine(), "cpus": os.cpu_count(),
            "open_file_limit": resource.getrlimit(resource.RLIMIT_NOFILE)[0]}


def compose(project, *args, data=None, timeout=120):
    cmd = ["docker", "compose", "--env-file", "/dev/null", "--project-name", project,
           "--file", str(HERE / "compose.yml"), *args]
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


def telemetry(project, nodes, stop, samples, errors):
    ids = {compose(project, "ps", "-q", role).decode().strip()[:12]: role for role in ROLES}
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
        except Exception as exc:
            errors.append(str(exc))
        samples.append(sample)
        stop.wait(4)


def seed(project, virtual, titles):
    output = compose(project, "exec", "-T", "db-b", "psql", "-X", "-qAt", "-U", "soundtime",
                     "-d", "soundtime", "-v", f"origins={virtual}", "-v", f"titles={titles}",
                     data=(HERE / "load-seed.sql").read_bytes(), timeout=1800)
    return json.loads(output.decode().strip().splitlines()[-1])


def mp3_fixture(project, node):
    encoded = compose(project, "exec", "-T", "backend-a", "ffmpeg", "-hide_banner", "-loglevel",
                      "error", "-i", "pipe:0", "-acodec", "libmp3lame", "-b:a", "64k", "-f",
                      "mp3", "pipe:1", data=wav_fixture(16000 * 40, 29))
    return node.upload("Browserwitness", encoded, extension="mp3", mime="audio/mpeg")


def churn(a, b, delay):
    time.sleep(delay)
    start = time.monotonic()
    track = a.upload("Loadcatalogchange", wav_fixture(32000, 47))
    wait_for("fresh title at B during load", lambda: b.search("Loadcatalogchange"), timeout=50)
    inserted = time.monotonic()
    a.call("/tracks/" + track, method="DELETE")
    wait_for("withdrawal at B during load", lambda: not b.search("Loadcatalogchange"), timeout=50)
    return {"visible_insert_ms": round((inserted - start) * 1000),
            "visible_withdraw_ms": round((time.monotonic() - inserted) * 1000)}


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
    report["seed"] = seed(project, virtual, titles)
    require(report["seed"] == {"virtual_catalogues": virtual, "virtual_tracks": virtual * titles,
                               "indexed_tracks": virtual * titles}, "Incomplete synthetic index")
    wait_for("metadata searchable after FTS reconciliation",
             lambda: b.call("/search?q=Quartz0001&include_p2p=false")["tracks"], timeout=120)
    cache = report["cache_before"] = b.call("/admin/storage/status")["cache"]
    if warm:
        require(cache["evictable_bytes"] >= sum(row["size"] for row in media), "Prewarming did not admit the audio")
    else:
        require(cache["maximum_bytes"] == 0, "Cold profile requires zero-disk admission")

    # Create regular listeners after the index build to keep their JWTs fresh.
    accounts = [b.account("loadlistener" + str(i)) for i in range(32 if full else 2)]
    pid = b.playlist(accounts[0], "Load witness", [witness])
    report["profile"]["distinct_listener_accounts"] = len(accounts)
    baseline = counters(nodes)
    report["container_samples"], report["monitor_errors"], report["browser_samples"] = [], [], []
    samples, errors, browsers = report["container_samples"], report["monitor_errors"], report["browser_samples"]
    stop = threading.Event()
    with ExitStack() as stack:
        browser = BrowserSession(stack, b, witness, "Browserwitness", [pid], [accounts[0]])
        thread = threading.Thread(target=telemetry, args=(project, nodes, stop, samples, errors), daemon=True)
        thread.start()

        def finish_telemetry():
            stop.set()
            thread.join(60)
        stack.callback(finish_telemetry)
        with subprocess.Popen(["node", str(HERE / "load-client.mjs")], stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE, text=True) as generator:
            try:
                generator.stdin.write(json.dumps({"base": b.url, "tokens": [a[1] for a in accounts],
                    "profile": args.mode, "virtual_catalogues": virtual, "media": media}) + "\n")
                generator.stdin.close()
                row = generator.stdout.readline()
                require(row and json.loads(row)["phase"] == "steady", "Generator failed during ramp")
                steady = time.monotonic()
                deadline = steady + (90 if full else 15)
                with ThreadPoolExecutor(max_workers=2) as executor:
                    mutation = executor.submit(churn, a, b, 10 if full else 2)
                    failure = executor.submit(pause_source, project, steady, full) if fault else None
                    while time.monotonic() + 3 < deadline and generator.poll() is None:
                        try:
                            obs = browser.phase("measure")
                            if time.monotonic() <= deadline:
                                browsers.append({"unix_s": time.time(), "search_to_screen_ms": obs["search_to_screen_ms"],
                                                 "first_media_progress_ms": obs["first_progress_ms"],
                                                 "seek_progress": obs["after_seek_seconds"]})
                        except Exception as exc:
                            report["browser_error"] = str(exc)
                            break
                    row = generator.stdout.readline()
                    require(row, "Missing generator result")
                    result = report["generator_result"] = json.loads(row)
                    require(generator.wait(timeout=15) == 0 and result["phase"] == "complete", "Generator failed")
                    report["live_catalogue_change"] = mutation.result(timeout=120)
                    if failure:
                        report["fault"] = failure.result(timeout=60)
            except BaseException:
                generator.kill()
                raise
    report["transport_delta"] = difference(baseline, counters(nodes))
    report["cache_after"] = b.call("/admin/storage/status")["cache"]
    for key in ("search_to_screen_ms", "first_media_progress_ms"):
        report[key] = quantiles([row[key] for row in browsers])
    report["container_peak_memory_bytes"] = {
        role: max((row["containers"].get(role, {}).get("memory_bytes") or 0 for row in samples), default=0)
        for role in ROLES}
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    report["orchestrator_peak_rss_bytes"] = peak if sys.platform == "darwin" else peak * 1024
    load = result["load"]
    all_readers = load["steady_samples"] > 0 and load["samples_at_target"] == load["steady_samples"]
    scheduled = load["dispatched_search_per_second"] >= (100 if full else 3) * .99
    exercised_origins = all(report["transport_delta"][name]["sent"] > 0 for name in ("A", "C"))
    report["workload_reached"] = bool(all_readers and scheduled and (warm or exercised_origins) and not result["omissions"])
    report["valid_browser_measurements"] = bool(browsers and "browser_error" not in report)
    report["screen_p95_within_300ms_in_sample"] = browsers != [] and report["search_to_screen_ms"]["p95"] <= 300
    report["status"] = "measured" if (report["workload_reached"] or fault) and report["valid_browser_measurements"] else "incomplete"
    if report["status"] == "incomplete":
        raise ScenarioError("Incomplete workload or browser observations; inspect report.json")


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--preflight":
        preflight(sys.argv[2])
        return
    parser = argparse.ArgumentParser()
    for name in ("a", "b", "c", "report"):
        parser.add_argument("--" + name, required=True)
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
        Path(args.report).write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
