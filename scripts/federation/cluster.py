"""Check live policy across public ingress, worker takeover, fencing and rollback."""
import argparse
from http.client import IncompleteRead
import json
import os
from pathlib import Path
import re
import subprocess
import time
from urllib.error import URLError
from urllib.request import urlopen
from uuid import UUID

from scenario import Instance, counters, difference, fetch_range, probe_stream, require, wait_for, wav_fixture


def healthy(base):
    try:
        with urlopen(base + "/healthz", timeout=1) as response:
            return response.status == 200
    except (URLError, TimeoutError, OSError):
        return False


def read_until_revoked(stream, audio, received):
    """Drain bytes already queued by the HTTP/proxy stack from the original response."""
    end = "eof"
    while received < len(audio):
        try:
            chunk = stream.read(min(65536, len(audio) - received))
        except IncompleteRead as exc:
            chunk, end = exc.partial, "incomplete_read"
        except ConnectionResetError:
            end = "connection_reset"
            break
        require(chunk == audio[received:received + len(chunk)], f"Incorrect audio at {received} during revocation")
        received += len(chunk)
        if not chunk or end == "incomplete_read":
            break
    require(received < len(audio), "Revoked response delivered the entire publication")
    return received, end


def cross_ingress_revocation(a, b, second_url, readers, observation):
    title = "Crossingpolicy"
    audio = wav_fixture(8 * 1024 * 1024, 19)
    source_id = b.upload(title, audio)
    a.connect(b, follow=True)
    track = wait_for("B's publication on A", lambda: a.search(title))
    account = a.account("policylistener")
    playlist = a.playlist(account, "Survives policy changes", [track])
    first, second = Instance("proxy-a-1", a.url), Instance("proxy-a-2", second_url)
    first.token = second.token = account[1]
    observation.update(origin_id=source_id, local_id=track, playlist=playlist,
                       ingress={first.name: first.url, second.name: second.url},
                       audio_admin_playlists_worker="backend-a",
                       catalogue_readers={r.name: r.url for r in readers})
    require(first.url != second.url and len({r.url for r in readers}) == 2, "Duplicate ingress or catalogue process")
    identities = observation["worker_identity_by_ingress"] = {
        proxy.name: proxy.call("/p2p/status")["node_id"] for proxy in (first, second)}
    require(all(identity == a.node_id for identity in identities.values()), "Ingress reached a different media owner")

    def projection(stage, state):
        results = stage.setdefault("catalogue_by_process", {})
        rows = stage.setdefault("playlist_by_ingress", {})
        for replica in readers:
            results[replica.name] = replica.search(title)
        for proxy in (first, second):
            rows[proxy.name] = proxy.positions(playlist)
        expected_hit = track if state == "playable" else None
        return (all(hit == expected_hit for hit in results.values()) and
                all(len(entries) == 1 and entries[0]["track_id"] == track and entries[0]["position"] == 0
                    and entries[0]["availability"] == state for entries in rows.values()))

    observation["initial"] = {}
    wait_for("both catalogue readers and playlist entry points", lambda: projection(observation["initial"], "playable"))
    for replica in readers:
        require(probe_stream(replica, track, audio) == 404, "A catalogue process unexpectedly served media")

    path = "/tracks/" + track + "/stream"
    for action, state, player in (("ban", "unavailable_on_instance", second), ("withdraw", "withdrawn", first)):
        stage = observation[action] = {"audio_ingress": player.name,
                                      "mutation_ingress": first.name if action == "ban" else "proxy-b"}
        before = counters([a, b])
        with player.open(path, headers={"Range": "bytes=0-"}) as stream:
            size = len(audio)
            require(stream.status == 206 and stream.headers.get("Content-Length") == str(size)
                    and stream.headers.get("Content-Range") == f"bytes 0-{size-1}/{size}", "Expected a complete HTTP range")
            prefix = stream.read(65536)
            require(prefix == audio[:65536], "Missing verified media before policy change")
            cut = difference(before, counters([a, b]))
            stage.update(expected_bytes=size, verified_prefix_bytes=len(prefix), transport_before_change=cut)
            require(len(prefix) <= cut["A"]["received"] < size and len(prefix) <= cut["B"]["sent"] < size
                    and cut["A"]["verified_windows"] > 0, "The remote response was not in flight at the policy change")
            started = time.monotonic()
            if action == "ban":
                ban = first.call("/admin/blocked-domains", method="POST", token=a.token,
                                 body={"domain": b.node_id, "reason": "cluster ingress fixture"})
                require(ban["domain"] == b.node_id, "Wrong peer banned")
            else:
                with b.open("/tracks/" + source_id, method="DELETE") as response:
                    require(response.status == 204, "Origin did not commit withdrawal")
            stage["mutation_ack_s"] = time.monotonic() - started
            received, end = read_until_revoked(stream, audio, len(prefix))
            stage.update(http_end=end, bytes_received=received, truncated_bytes=size-received,
                         response_ended_s=time.monotonic()-started)
        after = difference(before, counters([a, b]))
        stage["transport_after_close"] = after
        require(after["A"]["received"] < size and after["B"]["sent"] < size, "Revocation left a full upstream download")
        wait_for("both replicas hide the track and both ingress playlists preserve it", lambda: projection(stage, state))
        stage["catalogue_and_playlists_observed_s"] = time.monotonic() - started
        statuses = stage["new_stream_http"] = {proxy.name: probe_stream(proxy, track, audio) for proxy in (first, second)}
        require(all(s in ((403, 503) if action == "ban" else (410,)) for s in statuses.values()),
                f"Fresh session did not enforce {action}: {statuses}")
        if action == "ban":
            first.call("/admin/blocked-domains/" + str(UUID(ban["id"])), method="DELETE", token=a.token)
            a.connect(b, follow=True)
            restored = observation["unban"] = {}
            wait_for("same publication after unban on both readers", lambda: projection(restored, "playable"))


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

    def url(service, index=1, port=8080):
        address = compose("port", "--index", str(index), service, str(port))
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

    readers = [Instance(f"catalog-http-{index}", url("catalog-http", index)) for index in (1, 2)]
    replicas = [wait_for("catalogue replica", lambda: replica.search("Restoredidentity")) for replica in readers]
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

    policy = report["cross_ingress_policy"] = {}
    running = policy["running_processes_on_A"] = {
        service: len(compose("ps", "--status", "running", "-q", service).splitlines())
        for service in ("proxy-a", "catalog-http", "backend-a", "standby-a")}
    require(running == {"proxy-a": 2, "catalog-http": 2, "backend-a": 1, "standby-a": 0}, "Unexpected cluster topology")
    rate = policy["source_upload_limit_bytes_per_s"] = int(
        compose("exec", "-T", "backend-b", "printenv", "P2P_UPLOAD_BYTES_PER_SECOND"))
    require(rate == 524288, "Fixture supplier upload limit missing")
    cross_ingress_revocation(a, b, url("proxy-a", 2, 80), readers, policy)
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
    except Exception as error:
        result["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        arguments.report.write_text(json.dumps(result, indent=2) + "\n")
