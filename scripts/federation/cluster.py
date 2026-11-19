"""Exercise a worker takeover, a fenced write and rollback on one isolated instance."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import time
from urllib.error import URLError
from urllib.request import urlopen

from scenario import Instance, fetch_range, probe_stream, require, wait_for, wav_fixture


def healthy(base):
    try:
        with urlopen(base + "/healthz", timeout=1) as response:
            return response.status == 200
    except (URLError, TimeoutError, OSError):
        return False


def exercise(args, report):
    project = os.environ["SOUNDTIME_TEST_COMPOSE_PROJECT"]
    require(re.fullmatch(r"st-fed-[a-f0-9]{16}", project), "expected a disposable project")
    root = Path(__file__).parent
    cli = ["docker", "compose", "--env-file", "/dev/null", "-p", project,
           "-f", str(root / "compose.yml"), "-f", str(root / "cluster.yml")]

    def compose(*arguments):
        return subprocess.check_output([*cli, *arguments], text=True, timeout=90).strip()

    def sql(statement):
        return compose("exec", "-T", "db-a", "psql", "-XAtq", "-v", "ON_ERROR_STOP=1",
                       "-U", "soundtime", "-d", "soundtime", "-c", statement)

    def url(service, index=1):
        address = compose("port", "--index", str(index), service, "8080")
        require(re.fullmatch(r"127\.0\.0\.1:\d+", address), "expected a loopback endpoint")
        return "http://" + address

    a, b = Instance("A", args.a), Instance("B", args.b)
    admin = a.account("owner", admin=True)
    b.account("onb", admin=True)
    a.identify()
    b.identify()
    original_identity = a.node_id
    data = wav_fixture(1024 * 1024, 9)
    track = a.upload("Restoredidentity", data)
    active_data = wav_fixture(8 * 1024 * 1024, 13)
    active_track = a.upload("Continuousidentity", active_data)
    playlist = a.playlist(admin, "Preserved after failover", [track])

    replicas = []
    for index in (1, 2):
        replica = Instance(f"reader{index}", url("catalog-http", index))
        replicas.append(wait_for("catalogue replica", lambda: replica.search("Restoredidentity")))
    require(replicas == [track, track], "two catalogue replicas disagreed")

    # Pause the readiness marker on this disposable origin after a complete
    # catalogue has been received. No user action follows restoration of the marker.
    b.connect(a, follow=True)
    previous = wait_for("A's complete catalogue on B", lambda: b.search("Restoredidentity"))
    require(sql("SELECT complete FROM federation_event_bootstrap WHERE singleton") == "t",
            "source journal has not completed its first preparation")
    sql("UPDATE federation_event_bootstrap SET complete=false WHERE singleton")
    try:
        for method, route, body in [
            ("POST", "/admin/p2p/peers", {"node_id": a.node_id, "direct_addresses": a.addresses, "follow_catalog": True}),
            ("PUT", "/admin/p2p/peers/" + a.node_id + "/subscription", {"followed": True}),
        ]:
            with b.open(route, method=method, body=body) as response:
                require(response.status == 202 and "preparing" in json.load(response)["message"],
                        f"{method} did not confirm the saved subscription and pending journal")
        a.upload("Journalresume", wav_fixture(16000, 31))
        require(b.search("Restoredidentity") == previous and not b.search("Journalresume"),
                "receiver exposed a partial catalogue while the source was preparing")
        require(any(peer["node_id"] == a.node_id and peer["is_online"] for peer in b.call("/admin/p2p/peers")),
                "preparing a catalogue incorrectly marked its origin offline")
    finally:
        sql("UPDATE federation_event_bootstrap SET complete=true WHERE singleton")
    resumed = wait_for("automatic journal catch-up", lambda: b.search("Journalresume"), timeout=50)
    require(b.search("Restoredidentity") == previous, "resuming changed the previous track identity")
    report["catalogue_preparation"] = {"add_http": 202, "subscribe_http": 202, "preserved_track": previous,
                                       "automatically_received_track": resumed}

    a.call("/admin/blocked-domains", method="POST", body={"domain": b.node_id})
    a.call("/tracks/" + track, method="DELETE")
    require(probe_stream(a, track, data) == 410, "withdrawal on active owner")
    old = sql("SELECT 'st/'||generation||'/'||holder FROM federation_worker_lease")
    require(re.fullmatch(r"st/\d+/[a-f0-9-]{36}", old), "invalid current owner")
    old_generation = int(old.split("/")[1])
    compose("up", "-d", "--no-build", "standby-a")
    standby_url = url("standby-a")
    require(not healthy(standby_url), "standby served before acquiring the identity")

    # Leave a real response open when its producer dies. Resume at the exact
    # next byte through the original proxy after PostgreSQL grants the lease.
    with a.open("/tracks/" + active_track + "/stream", headers={"Range": "bytes=0-"}) as stream:
        prefix = stream.read(16384)
        require(prefix == active_data[:16384], "unexpected audio before takeover")
        compose("kill", "-s", "KILL", "backend-a")
        started = time.monotonic()
    wait_for("standby takeover", lambda: healthy(standby_url), timeout=45)
    wait_for("owner behind the original proxy URL", lambda: healthy(a.url), timeout=30)
    elapsed = time.monotonic() - started
    a.identify()
    generation = int(sql("SELECT generation FROM federation_worker_lease"))
    require(generation > old_generation and a.node_id == original_identity,
            "identity or generation changed incorrectly")
    remaining = len(active_data) - len(prefix)
    fetch_range(a, active_track, active_data, len(prefix), remaining)

    # Statement-level fencing must reject even a stale write that selects no rows.
    sql(f"""BEGIN;
        SET LOCAL application_name = '{old}';
        DO $$ BEGIN
            BEGIN
                UPDATE publication_offers SET expires_at = expires_at WHERE false;
                RAISE EXCEPTION 'stale worker was accepted';
            EXCEPTION WHEN SQLSTATE '55000' THEN NULL;
            END;
        END $$;
        ROLLBACK""")
    require(probe_stream(a, track, data) == 410, "rollback resurrected a withdrawal")
    rows = a.positions(playlist)
    require(len(rows) == 1 and rows[0]["track_id"] == track and rows[0]["position"] == 0
            and rows[0]["availability"] == "withdrawn", "lost saved position")
    require(any(peer["domain"] == b.node_id for peer in a.call("/admin/blocked-domains")),
            "rollback lost ban")
    require(not a.telemetry()["range_streaming"], "expected limited transport on replacement")
    report.update(status="accepted", track=track, reader_results=replicas, node_id=a.node_id,
                  previous_generation=old_generation, generation=generation, recovery_seconds=elapsed,
                  withdrawn_http_status=410, retained_position=rows[0]["position"],
                  resumed_at_byte=len(prefix), resumed_bytes=remaining)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("a", "b", "c", "mode"):
        parser.add_argument("--" + option, required=True)
    parser.add_argument("--report", type=Path, required=True)
    arguments = parser.parse_args()
    result = {"mode": arguments.mode, "status": "failed"}
    try:
        exercise(arguments, result)
    finally:
        arguments.report.write_text(json.dumps(result, indent=2) + "\n")
