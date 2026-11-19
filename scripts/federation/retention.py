"""Exercise a quota-backed local pin, public mirror, restart and owner withdrawal."""
import argparse
import json
from pathlib import Path
from urllib.error import HTTPError, URLError

from scenario import (Instance, ScenarioError, counters, difference, fetch_range, probe_stream,
                      require, switch_backend, wait_for, wav_fixture)


def ready(node):
    try:
        node.identify()
        return True
    except (ScenarioError, HTTPError, URLError, OSError):
        return False


def cache(node):
    return node.call("/admin/storage/status")["cache"]


def run(args, report):
    a, b, c = nodes = [Instance(k, getattr(args, k.lower())) for k in "ABC"]
    for node in nodes:
        require(not node.call("/setup/status")["has_admin"], "Retention needs fresh instances")
        node.account("admin" + node.name.lower(), admin=True)
        node.identify()
    audio = wav_fixture(4 * 1024 * 1024, 19)
    source = a.upload("Rareorigin", audio)
    independent = c.upload("Independentretention", audio)
    for x in nodes:
        for y in nodes:
            if x != y:
                x.connect(y, follow=True)
    b_id = wait_for("original on B", lambda: b.search("Rareorigin"))
    c_id = wait_for("original on C", lambda: c.search("Rareorigin"))
    endpoint = "/admin/cache/tracks/" + b_id + "/pin"
    before = cache(b)
    require(before["maximum_bytes"] > 0 and before["pinned_bytes"] == 0, "Invalid initial quota")
    fetch_range(b, b_id, audio, 2*1024*1024, 65536)
    require(cache(b)["evictable_bytes"] == before["evictable_bytes"], "A partial play persisted audio")
    fetch_range(b, b_id, audio, 0, len(audio))
    require(cache(b)["evictable_bytes"] >= len(audio), "Full play did not admit a verified copy")
    result = b.call(endpoint, method="PUT")
    require(result["pinned_publications"] == 1 and b.call(endpoint)["pinned"], "Pin was not recorded")
    report["after_pin"] = cache(b)
    require(report["after_pin"]["pinned_bytes"] >= len(audio), "Missing physical pin charge")
    identity = b.node_id
    switch_backend("B", False)
    switch_backend("B", True)
    wait_for("restarted B", lambda: ready(b), timeout=60)
    require(b.node_id == identity and b.call(endpoint)["pinned"], "Restart lost the public identity or pin")
    for x, y in [(b,a),(a,b),(c,b),(b,c)]:
        x.connect(y, follow=True)

    # Ban A on C to guarantee the fetch cannot accidentally select the origin;
    # then stop A to establish that B's durable copy works independently of A.
    ban = c.call("/admin/blocked-domains", method="POST", body={"domain":a.node_id,"reason":"retention test"})
    switch_backend("A", False)
    try:
        def from_mirror():
            try:
                fetch_range(c,c_id,audio,3*1024*1024,65536)
                return True
            except HTTPError:
                return False
        # A is stopped; only query live telemetry.
        live = [b,c]
        first = counters(live)
        wait_for("C playback from B with origin offline", from_mirror, timeout=60)
        report["mirror_only"] = difference(first,counters(live))
        require(report["mirror_only"]["B"]["sent"] >= 65536 and report["mirror_only"]["C"]["verified_windows"] > 0,
                "The durable mirror did not serve C")
        fetch_range(b,b_id,audio,131072,65536)
    finally:
        switch_backend("A", True)
    wait_for("restarted origin", lambda: ready(a), timeout=60)
    c.call("/admin/blocked-domains/" + ban["id"],method="DELETE")
    for x,y in [(a,b),(b,a),(a,c),(c,a)]:
        x.connect(y, follow=True)
    a.call("/tracks/" + source,method="DELETE")
    wait_for("pinned B rejects signed withdrawal",lambda:probe_stream(b,b_id,audio)==410)
    wait_for("C rejects signed withdrawal",lambda:probe_stream(c,c_id,audio)==410)
    fetch_range(c,independent,audio,0,64)
    b.call(endpoint,method="DELETE")
    wait_for("released physical pin",lambda:cache(b)["pinned_bytes"]==0)
    report["after_withdrawal"] = cache(b)
    report["status"] = "accepted"


def main():
    parser = argparse.ArgumentParser()
    for name in ["a","b","c","report","mode"]:
        parser.add_argument("--"+name,required=True)
    args=parser.parse_args()
    report={"mode":args.mode,"status":"failed"}
    try:
        require(args.mode=="retention","Unexpected test mode")
        run(args,report)
    except Exception as exc:
        report["error"] = str(exc)
        raise
    finally:
        Path(args.report).write_text(json.dumps(report,indent=2)+"\n")


if __name__=="__main__":
    main()
