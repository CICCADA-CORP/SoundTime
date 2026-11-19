"""Exercise a quota-backed local pin, public mirror, restart and owner withdrawal."""
import argparse
import json
import time
from pathlib import Path
from urllib.error import HTTPError, URLError

from cache_pressure import cache, pressure
from scenario import (Instance, ScenarioError, counters, difference, fetch_range, probe_stream,
                      require, switch_backend, wait_for, wav_fixture)


def ready(node):
    try:
        node.identify()
        return True
    except (ScenarioError, HTTPError, URLError, OSError):
        return False


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
                x.connect(y, follow=not (x is c and y is b))
    b_id = wait_for("original on B", lambda: b.search("Rareorigin"))
    c_id = wait_for("original on C", lambda: c.search("Rareorigin"))
    endpoint = "/admin/cache/tracks/" + b_id + "/pin"
    report["cache_pressure"] = pressure(a, c)
    before = cache(b)
    require(before["maximum_bytes"] > 0 and before["pinned_bytes"] == 0, "Invalid initial quota")
    fetch_range(b, b_id, audio, 2*1024*1024, 65536)
    require(cache(b)["evictable_bytes"] == before["evictable_bytes"], "A partial play persisted audio")
    fetch_range(b, b_id, audio, 0, len(audio))
    require(cache(b)["evictable_bytes"] >= len(audio), "Full play did not admit a verified copy")
    mirrors = "/admin/p2p/tracks/" + c_id + "/mirrors"
    wait_for("initial evictable offer on C", lambda: b.node_id in c.call(mirrors)["providers"], timeout=60)
    b.call("/admin/storage/cache", method="PUT", body={"maximum_bytes": 0})
    require(cache(b)["evictable_bytes"] == 0, "Quota reduction retained the unpinned audio")
    wait_for("C forgets B's evicted offer", lambda: b.node_id not in c.call(mirrors)["providers"], timeout=90)
    report["after_eviction"] = {"usage_B": cache(b), "providers_on_C": c.call(mirrors)["providers"]}
    b.call("/admin/storage/cache", method="PUT", body={"maximum_bytes": before["maximum_bytes"]})
    fetch_range(b, b_id, audio, 0, len(audio))
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
        x.connect(y, follow=not (x is c and y is b))

    # Ban A on C to guarantee the fetch cannot accidentally select the origin;
    # then stop A to establish that B's durable copy works independently of A.
    ban = c.call("/admin/blocked-domains", method="POST", body={"domain":a.node_id,"reason":"retention test"})
    wait_for("unfollowed B offered the pinned publication to C",
             lambda: b.node_id in c.call(mirrors)["providers"], timeout=60)
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
        # C now subscribes to B, pins the same publication via B and warms its
        # local reader. A stays blocked on C until after the withdrawal checks.
        c.subscribe(b, True)
        unfollow = c.call(f"/admin/p2p/peers/{a.node_id}/subscription",
                          method="PUT", body={"followed": False})
        require(unfollow["message"] == "Catalogue unfollowed locally (peer not notified)",
                "C did not persist its unsubscribe from the blocked origin")
        c_pin = "/admin/cache/tracks/" + c_id + "/pin"
        c.call(c_pin, method="PUT")
        require(c.call(c_pin)["pinned"] and cache(c)["pinned_bytes"] >= len(audio),
                "C did not retain the origin publication from its mirror")
        first = counters(live)
        fetch_range(c,c_id,audio,512*1024,65536)
        report["warm_local"] = difference(first,counters(live))
        require(report["warm_local"]["C"]["received"] == report["warm_local"]["B"]["sent"] == 0,
                "C's pin did not serve the warm read locally")
    finally:
        switch_backend("A", True)
    wait_for("restarted origin", lambda: ready(a), timeout=60)
    for x,y in [(a,b),(b,a)]:
        x.connect(y, follow=True)
    withdrawn_at = time.monotonic()
    a.call("/tracks/" + source,method="DELETE")
    wait_for("pinned B rejects signed withdrawal",lambda:probe_stream(b,b_id,audio)==410,timeout=90)
    elapsed_b = time.monotonic() - withdrawn_at
    wait_for("pinned C receives withdrawal through B",lambda:probe_stream(c,c_id,audio)==410,timeout=90)
    elapsed_c = time.monotonic() - withdrawn_at
    require(any(row["id"] == ban["id"] for row in c.call("/admin/blocked-domains")),
            "C lost its origin ban before the mirror-only withdrawal")
    require(b.node_id not in c.call(mirrors)["providers"],
            "Withdrawn publication still lists a playable mirror")
    fetch_range(c,independent,audio,0,64)
    report["hot_withdrawal"] = {"B_http": 410, "C_http": 410, "A_blocked_on_C": True,
                                "B_observed_s": elapsed_b, "C_observed_s": elapsed_c,
                                "C_followed_mirror": b.node_id, "independent_http": 206}
    c.call(c_pin, method="DELETE")
    b.call(endpoint,method="DELETE")
    wait_for("released physical pin",lambda:cache(b)["pinned_bytes"]==0)
    wait_for("released mirror-subscriber pin",lambda:cache(c)["pinned_bytes"]==0)
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
