"""Measure browser playback over disposable, independently calibrated P2P interfaces."""
import argparse
from contextlib import ExitStack
import ipaddress
import json
import math
import os
from pathlib import Path
import re
import select
import subprocess
import sys

from scenario import BrowserSession, Instance, ScenarioError, require, switch_backend, wait_for, wav_fixture


def run(command, timeout=30):
    result = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    require(not result.returncode, f"{command[0]} exited {result.returncode}: {result.stderr[-500:]}")
    return result.stdout.strip()


class Links:
    def __init__(self):
        self.project = os.environ.get("SOUNDTIME_TEST_COMPOSE_PROJECT", "")
        require(re.fullmatch(r"st-fed-[0-9a-f]{16}", self.project), "Expected a disposable federation Compose project")
        self.image = "soundtime-federation-link-tools:" + self.project
        self.network = self.project + "_peers"
        network = json.loads(run(["docker", "network", "inspect", self.network]))[0]
        labels = network["Labels"]
        require(network["Internal"] and network["Driver"] == "bridge"
                and labels.get("com.docker.compose.project") == self.project
                and labels.get("com.docker.compose.network") == "peers", "Expected the project's isolated peer bridge")
        self.nodes = {}
        for name in "ABC":
            service = "backend-" + name.lower()
            ids = run(["docker", "ps", "-q", "--no-trunc", "--filter", "label=com.docker.compose.project=" + self.project,
                       "--filter", "label=com.docker.compose.service=" + service]).splitlines()
            require(len(ids) == 1 and re.fullmatch(r"[a-f0-9]{64}", ids[0]), "Expected one running " + service)
            obj = json.loads(run(["docker", "container", "inspect", ids[0]]))[0]
            networks = obj["NetworkSettings"]["Networks"]
            require(set(networks) == {self.network, self.project + "_site-" + name.lower()}
                    and obj["HostConfig"]["NetworkMode"] != "host", "Unexpected backend networks")
            self.nodes[name] = {"id": ids[0], "ip": str(ipaddress.IPv4Address(networks[self.network]["IPAddress"]))}
        require(len({n["ip"] for n in self.nodes.values()}) == 3, "Non-unique backend addresses")
        self.devices = {}

    def command(self, name, *args, cap=None):
        base = ["docker", "run", "--rm", "--network", "container:" + self.nodes[name]["id"],
                "--read-only", "--cap-drop=ALL", "--security-opt=no-new-privileges:true"]
        if cap:
            base += ["--cap-add=" + cap]
        else:
            base += ["--user", "65534:65534"]
        return base + [self.image, *args]

    def apply(self, name):
        source = self.nodes[name]["ip"]
        devices = set()
        for peer in self.nodes.keys() - {name}:
            route = json.loads(run(self.command(name, "ip", "-j", "route", "get", self.nodes[peer]["ip"])))[0]
            require(route.get("prefsrc") == source and not route.get("gateway"), "Peer path is not a directly attached link")
            devices.add(route["dev"])
        require(len(devices) == 1, "All peer paths must use the same private interface")
        device = devices.pop()
        require(device != "lo" and re.fullmatch(r"[a-zA-Z0-9_.-]{1,15}", device), "Invalid peer interface")
        self.devices[name] = device
        addr = json.loads(run(self.command(name, "ip", "-j", "addr", "show", "dev", device)))[0]
        require(any(a["local"] == source for a in addr["addr_info"]), "Unverified peer interface address")
        old = json.loads(run(self.command(name, "tc", "-j", "qdisc", "show", "dev", device)))
        require(all(q.get("handle") in ("0:", "41:") and
                    (q.get("handle") != "41:" or q.get("kind") == "netem") for q in old), "Interface has another traffic policy")
        run(self.command(name, "tc", "qdisc", "replace", "dev", device, "root", "handle", "41:", "netem",
                         "limit", "256", "delay", "30ms", "rate", "10mbit", cap="NET_ADMIN"))
        config = run(self.command(name, "tc", "qdisc", "show", "dev", device))
        require("netem 41:" in config and "delay 30ms" in config and "rate 10Mbit" in config, "Wrong link configuration: " + config)
        return {"ipv4": source, "device": device, "qdisc": config}

    def counters(self, name):
        qdiscs = json.loads(run(self.command(name, "tc", "-s", "-j", "qdisc", "show", "dev", self.devices[name])))
        qdisc = next(q for q in qdiscs if q["handle"] == "41:")
        return {key: qdisc[key] for key in ("bytes", "packets", "drops")}

    def calibrate(self, src, dst):
        source, target = self.nodes[src]["ip"], self.nodes[dst]["ip"]
        ping = run(self.command(src, "ping", "-4", "-n", "-q", "-I", source, "-c", "7", "-i", "0.2", "-W", "2", target, cap="NET_RAW"))
        match = re.search(r"= ([\d.]+)/([\d.]+)/([\d.]+)/([\d.]+) ms", ping)
        require(match and "0% packet loss" in ping and "100% packet loss" not in ping, "RTT calibration failed: " + ping)
        low, mean, high, jitter = map(float, match.groups())
        # Each calibration server terminates after its one test or its idle deadline.
        command = self.command(dst, "iperf3", "-s", "-1", "-p", "15201", "-B", target, "--forceflush", "--idle-timeout", "12")
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as server:
            require(select.select([server.stdout], [], [], 10)[0], "Bandwidth probe did not start")
            ready = b""
            while b"Server listening on" not in ready:
                chunk = os.read(server.stdout.fileno(), 4096)
                require(chunk, "Bandwidth probe failed to listen")
                ready += chunk
            result = json.loads(run(self.command(src, "iperf3", "-4", "-c", target, "-p", "15201", "-B", source,
                                               "-t", "3", "-O", "1", "-J"), timeout=15))
            require(server.wait(timeout=10) == 0, "Bandwidth probe server exited unsuccessfully")
        mbps = result["end"]["sum_received"]["bits_per_second"] / 1_000_000
        require(55 <= mean <= 90 and 7.5 <= mbps <= 10.7,
                f"{src} to {dst}: unexpected calibrated RTT {mean:.1f} ms or throughput {mbps:.2f} Mbit/s")
        return {"rtt_ms": {"min": low, "mean": mean, "max": high, "jitter": jitter}, "tcp_payload_mbit_s": mbps}


def quantiles(values):
    ordered = sorted(values)
    return {"samples": len(values), **{f"p{p}_ms": ordered[math.ceil(len(ordered) * p / 100) - 1] for p in (50, 95, 99)}}


def trial(browser, source, b, warm):
    before = b.telemetry()
    sent = source.telemetry()["audio_bytes_sent"]
    sample = browser.phase("first_sound")
    after = b.telemetry()
    sample.update({"origin": source.name, "range_handshakes": after["range_connections_established"] - before["range_connections_established"],
                   "verified_windows": after["range_windows_verified"] - before["range_windows_verified"],
                   "received_bytes": after["audio_bytes_received"] - before["audio_bytes_received"],
                   "origin_sent_bytes": source.telemetry()["audio_bytes_sent"] - sent})
    require(sample["range_handshakes"] == (0 if warm else 1), "Unexpected media connection reuse: " + str(sample))
    require(min(sample["verified_windows"], sample["received_bytes"], sample["origin_sent_bytes"]) > 0,
            "Browser did not fetch verified remote audio")
    return sample


def benchmark(args, report):
    links = Links()
    report["topology"] = {name: links.apply(name) for name in "ABC"}
    report["calibration"] = calibration = {}
    for src in "ABC":
        for dst in "ABC":
            if src != dst:
                calibration[src + "_to_" + dst] = links.calibrate(src, dst)
    peers = {name: Instance(name, getattr(args, name.lower())) for name in "ABC"}
    b = peers["B"]
    users, playlists, tracks = {}, {}, {}
    for name, peer in peers.items():
        status = peer.call("/setup/status")
        require(not status["has_admin"] and not status["setup_complete"], "Expected an empty disposable site")
        peer.account("shapedadmin" + name.lower(), admin=True)
        peer.identify()
        require(peer.telemetry()["explicit_catalog"] and not peer.relay_only, "Expected a modern direct peer")
        peer.addresses = [links.nodes[name]["ip"] + ":11204"]
    ids = {name: n.node_id for name, n in peers.items()}
    require(len(set(ids.values())) == 3, "Duplicate identities")
    report["nodes"] = ids
    for i, name in enumerate(("A", "C"), 1):
        peer = peers[name]
        title = "Controlled" + name
        # Stream an audible fixture from each of the other peers. The reader has zero disk quota.
        peer.upload(title, wav_fixture(2 * 1024 * 1024, 7 + i))
        b.connect(peer, follow=True)
        remote = wait_for(title + " in reader index", lambda: b.search(title))
        users[name] = b.account("shapedlistener" + name.lower())
        tracks[name] = remote
        playlists[name] = b.playlist(users[name], "Listener 1", [remote])
    with ExitStack() as stack:
        browsers = {name: BrowserSession(stack, b, tracks[name], "Controlled" + name, [playlists[name]], [users[name]]) for name in ("A", "C")}
        report["cold"] = cold = []
        report["warm"] = warm = []
        for batch in range(5):
            name = ("A", "C")[batch % 2]
            if batch:
                switch_backend("B", False)
                switch_backend("B", True)
                def reader_ready():
                    try:
                        b.identify()
                        return b.node_id == ids["B"]
                    except (OSError, ScenarioError):
                        return False
                wait_for("restarted reader identity", reader_ready, timeout=60)
                # Restart can replace the network namespace; verify its private interface again.
                report["topology"]["B"] = links.apply("B")
                b.addresses = [links.nodes["B"]["ip"] + ":11204"]
                b.connect(peers[name], follow=True)
            require(b.telemetry()["range_connections_established"] == 0, "Media connection established before cold trial")
            cold.append(trial(browsers[name], peers[name], b, False))
            for _ in range(4):
                warm.append(trial(browsers[name], peers[name], b, True))
        report["first_sound"] = {k: quantiles([v["first_sound_ms"] for v in values]) for k, values in (("cold", cold), ("warm", warm))}
    report["egress"] = {name: links.counters(name) for name in "ABC"}
    report["warm_p95_under_2s"] = report["first_sound"]["warm"]["p95_ms"] < 2000
    report["status"] = "measured"


def main():
    parser = argparse.ArgumentParser()
    for field in ("a", "b", "c", "report"):
        parser.add_argument("--" + field, required=True)
    parser.add_argument("--mode", choices=["shaped"], required=True)
    args = parser.parse_args()
    report = {"mode": "shaped", "status": "failed", "target_mbit_s": 10, "target_rtt_ms": 60}
    try:
        benchmark(args, report)
    except Exception as error:
        report["error"] = str(error)
        print(error, file=sys.stderr)
    finally:
        Path(args.report).write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["status"] == "measured" else 1


if __name__ == "__main__":
    sys.exit(main())
