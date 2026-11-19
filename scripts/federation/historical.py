"""Cross-version catalogue exchange with the unmodified historical Rust backend."""
import argparse
import json
import os
from pathlib import Path
import time
from urllib.error import URLError
from urllib.parse import urlencode
from uuid import UUID

from historical_image import BUILD_MARKER, inspect_c
from scenario import (Instance, ScenarioError, counters, difference, inspect_credit_pair,
                      publish_credit_pair, require, wait_for, wav_fixture)


def public_track(node, title):
    query = urlencode({"q": title, "include_p2p": "false"})
    tracks = node.call("/search?" + query, token="")["tracks"]
    exact = [str(UUID(track["id"])) for track in tracks if track["title"] == title]
    require(len(exact) < 2, f"{node.name} duplicated the imported title {title}")
    if not exact:
        return None
    detail = node.call("/tracks/" + exact[0], token="")
    require(detail["id"] == exact[0] and detail["title"] == title and detail["duration_secs"] > 0,
            f"{node.name} returned inconsistent public search and track records")
    return exact[0]


def old_identity(node):
    # This version publishes only an address count. Its numeric route is
    # established independently using the verified disposable container.
    try:
        status = node.call("/p2p/status", token="")
    except (URLError, ScenarioError):
        return None
    if not status.get("enabled") or not status.get("node_id") or status.get("direct_addresses", 0) < 1:
        return None
    require(not status["relay_connected"] and not status["dht_discovery_enabled"]
            and not status["rendezvous_configured"], "Historical node obtained an external route")
    return status["node_id"]


def initial_archived_ping(origin, peer):
    attempts = 0

    def dial():
        nonlocal attempts
        attempts += 1
        reply = origin.call(f"/admin/p2p/peers/{peer.node_id}/ping", method="POST")["message"]
        if reply.casefold() == "ping failed: connection error: no addressing information available":
            return False
        require(reply.startswith("pong received: Pong {") and peer.node_id in reply,
                f"{origin.name}: unexpected historical Pong for {peer.name}: {reply}")
        return True

    wait_for("initial historical mDNS route", dial, timeout=8)
    return attempts


def run(args, report):
    a, b, c = nodes = [Instance(letter, getattr(args, letter.lower())) for letter in "ABC"]
    report["historical_backend"] = inspect_c(os.environ["SOUNDTIME_TEST_COMPOSE_PROJECT"],
                                             os.environ["SOUNDTIME_HISTORICAL_C"])
    for node in nodes:
        require(not node.call("/setup/status")["has_admin"], "Historical recipe needs fresh databases")
        node.account("admin" + node.name.lower(), admin=True)
    for node in (a, b):
        node.identify()
        require(node.telemetry()["explicit_catalog"], "A and B must run the current explicit protocol")
    c.node_id = wait_for("historical public node status", lambda: old_identity(c), timeout=60)
    c.addresses = [report["historical_backend"]["direct_address"]]
    require(len({node.node_id for node in nodes}) == 3, "Instances shared a cryptographic identity")
    report["node_ids"] = {node.name: node.node_id for node in nodes}
    a.connect(b, follow=False)
    b.connect(a, follow=False)

    modern = "Currentpublisher"
    historic = "Archivepublisher"
    modern_id = b.upload(modern, wav_fixture(32_000, 13))
    old_id = c.upload(historic, wav_fixture(48_000, 17))
    require(public_track(b, modern) == modern_id and public_track(c, historic) == old_id,
            "Original public catalogue entries are missing")

    publish_credit_pair(b)
    before = counters([b])
    refused = b.add_peer(c, follow=False)
    require("legacy peer requires explicit follow" in refused["message"],
            "The modern node did not withhold its catalogue-producing v1 ping")
    # The older HTTP route accepts only an ID. The original dialler may resolve
    # B over mDNS or use the route learned on the preceding QUIC connection.
    report["initial_mdns_dials"] = initial_archived_ping(c, b)
    time.sleep(1)
    require(not public_track(b, historic) and not public_track(c, modern),
            "The catalogues crossed the unfollowed boundary")
    quiet = difference(before, counters([b]))["B"]
    require(quiet["catalog_entries"] == quiet["sent"] == quiet["received"] == 0,
            "The modern node received a catalogue or audio before explicit follow")
    report["before_follow_B"] = quiet

    b.subscribe(c, True)
    reply = b.call("/admin/p2p/peers/" + c.node_id + "/ping", method="POST")["message"]
    require(c.node_id in reply and BUILD_MARKER in reply and "Pong" in reply,
            "The authenticated v1 Pong did not come from the archived build")
    # This independent old-to-new v1 Ping now requests B's catalogue.
    c.ping(b)
    from_c = wait_for("historical catalogue on B", lambda: public_track(b, historic))
    from_b = wait_for("current catalogue on C", lambda: public_track(c, modern))
    report["credit_interop"] = {"B": inspect_credit_pair(b), "C": inspect_credit_pair(c, archived=True)}
    exchanged = difference(before, counters([b]))["B"]
    require(exchanged["catalog_entries"] > 0 and exchanged["sent"] == exchanged["received"] == 0,
            "Expected metadata-only interchange on the modern participant")
    require(not public_track(a, historic) and not public_track(a, modern),
            "Unsubscribed modern instance imported a third-party catalogue")
    require(old_identity(c) == c.node_id, "Historical runtime lost its verified isolated transport")
    report.update({"build_pong": reply, "after_follow_B": exchanged,
                   "C_to_B": {"source": old_id, "indexed": from_c},
                   "B_to_C": {"source": modern_id, "indexed": from_b}, "status": "accepted"})


def main():
    parser = argparse.ArgumentParser()
    for arg in ("a", "b", "c", "report"):
        parser.add_argument("--" + arg, required=True)
    parser.add_argument("--mode", choices=["historical"], required=True)
    args = parser.parse_args()
    report = {"mode": args.mode, "status": "failed"}
    try:
        run(args, report)
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        Path(args.report).write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
