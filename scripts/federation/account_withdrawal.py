"""Check a regular uploader's account deletion as two independent playlist owners."""
import json
import os
from pathlib import Path
import re
import subprocess
from urllib.error import HTTPError, URLError
from uuid import UUID

from scenario import require, switch_backend, wait_for, wav_fixture


def database(node, playlist, track, creator):
    require(node.name in ("A", "B"), "Unexpected account withdrawal database")
    project = os.getenv("SOUNDTIME_TEST_COMPOSE_PROJECT", "")
    psql = ["psql", "-XqAt", "-v", "ON_ERROR_STOP=1"]
    env = os.environ.copy()
    if re.fullmatch(r"st-fed-[a-f0-9]{16}", project):
        command = ["docker", "compose", "--env-file", "/dev/null", "-p", project,
                   "-f", str(Path(__file__).with_name("compose.yml")), "exec", "-T",
                   "db-" + node.name.lower(), *psql, "-U", "soundtime", "-d", "soundtime"]
    else:
        dsn = os.getenv(f"SOUNDTIME_TEST_DATABASE_{node.name}_URL")
        require(os.getenv("SOUNDTIME_TEST_FAULT_DRIVER") and dsn,
                f"Native acceptance requires the fixture database URL for {node.name}")
        env["PGDATABASE"] = dsn
        command = psql
    sql = """SELECT json_build_object(
        'track',t.id,'origin',p.origin_publication_id,'issuer',c.node_id,'revision',p.revision,
        'state',p.state,'uploader',t.uploaded_by,'creator_present',EXISTS(SELECT FROM users WHERE id=:'creator'),
        'playlist',l.id,'owner',l.user_id,'entries',(SELECT json_agg(json_build_array(position,track_id)
          ORDER BY position) FROM playlist_tracks WHERE playlist_id=l.id))
        FROM tracks t JOIN track_publications p ON p.track_id=t.id
        JOIN federation_catalogs c ON c.id=p.catalog_id CROSS JOIN playlists l
        WHERE t.id=:'track' AND l.id=:'playlist';"""
    for key, value in [("track", track), ("playlist", playlist), ("creator", creator)]:
        command.extend(["-v", f"{key}={UUID(value)}"])
    row = subprocess.check_output(command, input=sql, env=env, text=True, timeout=20).strip()
    require(row, f"{node.name}: publication or personal playlist disappeared from PostgreSQL")
    return json.loads(row)


def response_status(node, path, expected, **request):
    try:
        with node.open(path, **request) as response:
            status = response.status
            data = response.read()
    except HTTPError as error:
        with error:
            status, data = error.code, b""
    require(status == expected, f"{node.name} {path} returned HTTP {status}, expected {expected}")
    return data


def exercise(a, b, remote_reader, anchor_a, anchor_b, report):
    creator = a.account("temporarypublisher")
    local_reader = a.account("localplaylistowner")
    require(len({creator[0], local_reader[0], remote_reader[0]}) == 3, "Duplicate test identities")
    require(a.call("/auth/me", token=creator[1])["role"] == "user", "Uploader must be an ordinary user")
    title, audio = "Departingpublisher", wav_fixture(48000, 43)
    original = a.upload(title, audio, token=creator[1])
    imported = wait_for("subscriber's copy of uploader metadata", lambda: b.search(title))
    listeners = [(a, local_reader, original, [anchor_a, original]),
                 (b, remote_reader, imported, [imported, anchor_b])]
    playlists = {n.name: n.playlist(user, "Account deletion " + n.name, order)
                 for n, user, _, order in listeners}

    def snapshots(withdrawn):
        result = {}
        for node, (user, token), track, order in listeners:
            pid = playlists[node.name]
            ordered = [[i, tid] for i, tid in enumerate(order)]
            state = "withdrawn" if withdrawn else "playable"
            rows = node.positions(pid, token=token)
            require([[r["position"], r["track_id"]] for r in rows] == ordered and
                    all(r["availability"] == (state if r["track_id"] == track else "playable") for r in rows),
                    f"{node.name}: personal playlist lost an entry, position or availability")
            detail = node.call("/tracks/" + track, token=token)
            require((detail["id"], detail["title"], detail["availability"]) == (track, title, state),
                    f"{node.name}: track details disagree with the playlist")
            status = 410 if withdrawn else 206
            body = response_status(node, f"/tracks/{track}/stream", status, token=token,
                                   headers={"Range": "bytes=0-63"})
            require(withdrawn or body == audio[:64], f"{node.name}: incorrect audio before withdrawal")
            row = database(node, pid, track, creator[0])
            require((row["playlist"], row["owner"], row["entries"]) == (pid, user, ordered) and
                    (row["track"], row["origin"], row["issuer"]) == (track, original, a.node_id) and
                    row["state"] == ("withdrawn" if withdrawn else "active"),
                    f"{node.name}: PostgreSQL identities, references or publication state changed")
            if node is a:
                require(row["creator_present"] == (not withdrawn) and
                        row["uploader"] == (None if withdrawn else creator[0]),
                        "Publisher account or upload ownership disagrees with its lifecycle")
            result[node.name] = {"database": row, "playlist_api": rows, "stream_status": status}
        return result

    response_status(a, "/tracks/" + original, 403, method="DELETE", token=local_reader[1])
    report["third_party_withdrawal_http"] = 403
    report["before"] = snapshots(False)
    response_status(a, "/auth/account", 204, method="DELETE", token=creator[1],
                    body={"password": a.login_credentials[creator[0]]["password"]})
    report["creator_delete_http"] = 204
    wait_for("account withdrawal on subscriber",
             lambda: b.call("/tracks/" + imported, token=remote_reader[1])["availability"] == "withdrawn")
    report["after_delete"] = snapshots(True)
    revision = report["after_delete"]["A"]["database"]["revision"]
    require(report["after_delete"]["B"]["database"]["revision"] == revision and
            all(revision > entry["database"]["revision"] for entry in report["before"].values()),
            "Withdrawal did not advance the same revision on A and B")

    def api_ready(node):
        try:
            with node.open("/p2p/status") as response:
                status = json.load(response)
                return status.get("enabled") and status.get("node_id") == node.node_id
        except (URLError, OSError):
            return False

    for node in (a, b):
        switch_backend(node.name, False)
        switch_backend(node.name, True)
        wait_for(f"{node.name} to resume with its identity", lambda: api_ready(node), timeout=60)
    report["after_restart"] = snapshots(True)
    require(report["after_restart"] == report["after_delete"],
            "Restart changed a withdrawn UUID, revision, playlist, stream status or personal reference")
