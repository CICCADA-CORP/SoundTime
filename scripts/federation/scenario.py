"""Exercise three fresh SoundTime instances through their public and admin HTTP APIs."""

import argparse
import base64
from contextlib import ExitStack, suppress
from datetime import datetime, timezone
import hashlib
import io
import json
import os
from pathlib import Path
import re
import secrets
import select
import stat
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError
from urllib.parse import urlencode, urlsplit
from urllib.request import Request, urlopen
from uuid import UUID
import wave


class ScenarioError(Exception):
    """The test environment or an already delivered contract failed."""


def require(condition, detail):
    if not condition:
        raise ScenarioError(detail)


class Instance:
    def __init__(self, name, url):
        self.name = name
        self.url = url.rstrip("/")
        self.token = None
        self.node_id = None
        self.addresses = []
        self.login_credentials = {}

    def open(self, path, method="GET", body=None, headers=None, token=None):
        headers = dict(headers or {})
        bearer = self.token if token is None else token
        if bearer:
            headers["Authorization"] = "Bearer " + bearer
        if isinstance(body, dict):
            body = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        request = Request(self.url + "/api" + path, method=method, data=body, headers=headers)
        return urlopen(request, timeout=40)

    def call(self, path, **kwargs):
        try:
            with self.open(path, **kwargs) as response:
                raw = response.read()
                if not raw or "application/json" not in response.headers.get("Content-Type", ""):
                    return None
                return json.loads(raw)
        except HTTPError as exc:
            with exc:
                reason = exc.read(512).decode(errors="replace")
            raise ScenarioError(f"{self.name} {path}: HTTP {exc.code}: {reason}") from exc

    def account(self, username, admin=False):
        # A fresh credential on each run; neither passwords nor tokens enter the report.
        payload = {"username": username, "email": f"{username}@example.test",
                   "password": secrets.token_urlsafe(24)}
        path = "/setup/admin" if admin else "/auth/register"
        result = self.call(path, method="POST", body=payload)
        account_id = str(UUID(result["user"]["id"]))
        token = result["tokens"]["access_token"]
        require(self.call("/auth/me", token=token)["id"] == account_id, "Unexpected account")
        if not admin:
            self.login_credentials[account_id] = {"username": username, "password": payload["password"]}
        if admin:
            self.token = token
            self.refresh_token = result["tokens"]["refresh_token"]
            self.call("/setup/instance", method="POST",
                      body={"instance_name": "Test " + self.name, "instance_description": "Federation test"})
            self.call("/setup/complete", method="POST",
                      body={"p2p_enabled": True, "open_registrations": True, "max_upload_size_mb": 32})
        return account_id, token

    def telemetry(self):
        return self.call("/admin/p2p/transport")

    def identify(self):
        status = self.call("/p2p/status")
        require(status["enabled"] and status["node_id"], f"{self.name}: P2P disabled")
        self.node_id = status["node_id"]
        transport = self.telemetry()
        self.relay_only = transport["relay_only"]
        if self.relay_only:
            url = urlsplit(status.get("relay_url") or "")
            require(status["relay_connected"] and (url.hostname or "").rstrip(".") == "relay.federation.test"
                    and url.port == 3340 and not transport["direct_addresses"],
                    f"{self.name}: expected only the local test relay")
        override = os.getenv(f"SOUNDTIME_TEST_{self.name}_DIRECT")
        self.addresses = override.split(",") if override else transport["direct_addresses"]
        require((not self.addresses) if self.relay_only else bool(self.addresses), f"{self.name}: unexpected direct sockets")

    def add_peer(self, other, follow=None):
        payload = {"node_id": other.node_id, "direct_addresses": other.addresses}
        if follow is not None:
            payload["follow_catalog"] = follow
        return self.call("/admin/p2p/peers", method="POST", body=payload)

    def connect(self, other, follow=None):
        self.add_peer(other, follow)
        peers = self.call("/admin/p2p/peers")
        require(any(p["node_id"] == other.node_id and p["is_online"] for p in peers),
                f"{self.name}: requested peer {other.name} is offline")
        # The existing add-peer route also returns HTTP 200 when the dial failed.
        self.ping(other)

    def ping(self, other):
        pong = self.call(f"/admin/p2p/peers/{other.node_id}/ping", method="POST")
        require("pong" in pong["message"].lower() and other.node_id in pong["message"],
                f"{self.name}: unverified Pong for {other.name}: {pong['message']}")

    def subscribe(self, other, followed):
        result = self.call(f"/admin/p2p/peers/{other.node_id}/subscription",
                           method="PUT", body={"followed": followed})
        expected = "Catalogue followed" if followed else "Catalogue unfollowed"
        require(result["message"] == expected, f"{self.name}: {result['message']}")

    def upload(self, title, pcm, extension="wav", mime="audio/wav", *, token=None, artist="SoundTime fixture", album=None):
        boundary = secrets.token_hex(24)
        fields = [
            f'--{boundary}\r\nContent-Disposition: form-data; name="title"\r\n\r\n{title}\r\n'.encode(),
            (f'--{boundary}\r\nContent-Disposition: form-data; name="artist"\r\n\r\n{artist}\r\n'.encode()
             if artist is not None else b""),
            (f'--{boundary}\r\nContent-Disposition: form-data; name="album"\r\n\r\n{album}\r\n'.encode()
             if album is not None else b""),
            (f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="{title}.{extension}"'
             f'\r\nContent-Type: {mime}\r\n\r\n').encode(),
            pcm, f"\r\n--{boundary}--\r\n".encode(),
        ]
        result = self.call("/upload", method="POST", body=b"".join(fields), token=token,
                           headers={"Content-Type": f"multipart/form-data; boundary={boundary}"})
        require(result["title"] == title and result["duration"] > 0, f"{self.name}: unusable WAV {title}")
        return str(UUID(result["id"]))

    def search(self, title):
        hits = self.call("/search?" + urlencode({"q": title, "include_p2p": "false"}))["tracks"]
        exact = [str(UUID(hit["id"])) for hit in hits if hit["title"] == title]
        require(len(exact) <= 1, f"{self.name}: ambiguous search for {title}")
        return exact[0] if exact else None

    def playlist(self, account, name, tracks):
        account_id, token = account
        playlist = self.call("/playlists", method="POST", token=token,
                             body={"name": name, "is_public": True})
        require(playlist["user_id"] == account_id, f"{name}: wrong playlist owner")
        for position, track_id in enumerate(tracks):
            self.call(f"/playlists/{playlist['id']}/tracks", method="POST", token=token,
                      body={"track_id": track_id, "position": position})
        return str(UUID(playlist["id"]))

    def positions(self, playlist_id, *, token=None):
        tracks = self.call("/playlists/" + playlist_id, token=token)["tracks"]
        # Older responses expose position only through the ordering of the array.
        return [{"position": t.get("position", i), "track_id": t["id"], "title": t["title"],
                 "availability": t.get("availability"), "artist_name": t.get("artist_name"),
                 "album_title": t.get("album_title")} for i, t in enumerate(tracks)]


def wav_fixture(frames, seed):
    """A deterministic audible 8-bit mono WAV, with content independent from its title."""
    out = io.BytesIO()
    period = bytes((i * seed % 127) + 64 for i in range(256))
    with wave.open(out, "wb") as writer:
        writer.setparams((1, 1, 16000, 0, "NONE", "not compressed"))
        writer.writeframes((period * ((frames + 255) // 256))[:frames])
    return out.getvalue()


def reader_volume(signatures):
    """Inspect the disposable reader's media volume without opening its identity key."""
    project = os.getenv("SOUNDTIME_TEST_COMPOSE_PROJECT", "")
    if re.fullmatch(r"st-fed-[0-9a-f]{16}", project):
        cmd = ["docker", "compose", "--env-file", "/dev/null", "-p", project,
               "-f", str(Path(__file__).with_name("compose.yml")), "exec", "-T",
               "--workdir", "/data", "backend-b"]
        cwd = None
        listing = subprocess.check_output(cmd + ["find", ".", "-printf", r"%y %s %P\0"], timeout=20)
        entries = [record.decode().split(" ", 2) for record in listing.split(b"\0") if record]
    else:
        root = os.getenv("SOUNDTIME_TEST_READER_VOLUME_B")
        require(os.getenv("SOUNDTIME_TEST_FAULT_DRIVER") and root,
                "Native acceptance needs SOUNDTIME_TEST_READER_VOLUME_B for B's disposable data root")
        cwd, cmd = Path(root).resolve(strict=True), []
        entries = []
        for path in cwd.rglob("*"):
            info = path.lstat()
            kind = "d" if stat.S_ISDIR(info.st_mode) else "f" if stat.S_ISREG(info.st_mode) else "other"
            entries.append((kind, info.st_size, str(path.relative_to(cwd))))

    files, partials = {}, []
    for kind, size, name in entries:
        require(kind in ("f", "d") and "\n" not in name, "Unexpected entry on disposable reader volume")
        path = Path(name)
        if (any(p.startswith((".part-", ".evict-")) for p in path.parts)
                or name.startswith("p2p-blobs/temp/") or path.suffix in (".sizes4", ".bitfield")
                or (name.startswith("p2p-blobs/cache-v2/") and path.name != ".owner")):
            partials.append(name)
        if kind == "f":
            files[name] = int(size)
    db, owner, key = "p2p-blobs/blobs.db", "p2p-blobs/cache-v2/.owner", "secret_key"
    require(db in files and owner in files and files.get(key, 0) > 0, "Incomplete reader volume")
    data, metadata, proofs = {}, {}, {}
    for name, size in sorted(files.items()):
        if name in (db, owner, key, "p2p-blobs/.soundtime-blobs-0.105"):
            metadata[name] = size
        elif name.endswith(".obao4") or Path(name).name == "proof":
            proofs[name] = size
        else:
            data[name] = size

    # Small blobs can live inline in redb. Inspect the actual bytes of every regular
    # file except the identity key, including the database and any proof sidecars.
    # Each fixture repeats its own 256-byte period throughout the WAV payload.
    require(signatures and all(s and b"\n" not in s for s in signatures), "Invalid fixture PCM signatures")
    scan = subprocess.run(cmd + ["env", "LC_ALL=C", "grep", "-Fl", "-f", "-", "--",
                                 *sorted(n for n in files if n != key)],
                          cwd=cwd, input=b"\n".join(signatures) + b"\n", capture_output=True, timeout=20)
    require(scan.returncode in (0, 1), f"Reader volume scan failed: {scan.stderr.decode(errors='replace')}")
    return {"data_files": data, "metadata_files": metadata, "proof_files": proofs,
            "partial_paths": sorted(partials), "remote_pcm_files": scan.stdout.decode().splitlines()}


def observe_zero_cache(reader, signatures, observations, phase):
    observation = observations[phase] = reader_volume(signatures)
    usage = reader.call("/admin/storage/status")["cache"]
    observation["quota"] = {k: usage[k] for k in ("maximum_bytes", "reserved_bytes", "pinned_bytes", "evictable_bytes")}
    require(all(v == 0 for v in observation["quota"].values()), f"B has a nonzero cache budget or allocation at {phase}")
    require(not observation["partial_paths"] and not observation["remote_pcm_files"],
            f"B has a partial file or persisted remote fixture audio at {phase}")
    baseline = observations["before_remote_play"]["data_files"]
    require(any(n.startswith("music/") for n in baseline), "B's local originals are absent from the volume")
    require(observation["data_files"] == baseline, f"B's media files changed at {phase}")


def wait_for(label, probe, timeout=40):
    deadline = time.monotonic() + timeout
    while True:
        value = probe()
        if value:
            return value
        if time.monotonic() >= deadline:
            raise ScenarioError(f"Timed out waiting for {label}")
        time.sleep(0.25)


def counters(instances):
    readings = {}
    for node in instances:
        status = node.telemetry()
        readings[node.name] = {key: status["audio_bytes_" + key] for key in ("sent", "received")}
        readings[node.name]["catalog_entries"] = status["catalog_entries_received"]
        readings[node.name]["proof_bytes"] = status["range_proof_bytes_received"]
        readings[node.name]["verified_windows"] = status["range_windows_verified"]
        readings[node.name]["relay_windows"] = status["relay_windows_verified"]
        require(all(type(n) is int and n >= 0 for n in readings[node.name].values()),
                f"Invalid transport counters on {node.name}")
    return readings


def difference(before, after):
    delta = {node: {key: end - before[node][key] for key, end in counts.items()}
             for node, counts in after.items()}
    require(all(n >= 0 for row in delta.values() for n in row.values()),
            "Transport counters reset during the test")
    return delta


def no_audio(delta):
    return all(row["sent"] == 0 and row["received"] == 0 for row in delta.values())


def quiet_catalogue(nodes, before, measurements, key):
    # v1 sends CatalogSync on a new stream after replying Pong. Inspect both the
    # actual wire counters (including rejected entries) and the local index.
    window = 2
    time.sleep(window)
    delta = difference(before, counters(nodes))
    measurements[key] = {"observation_s": window, "p2p": delta}
    require(all(row["catalog_entries"] == 0 for row in delta.values()) and no_audio(delta),
            f"Unexpected catalogue or audio in {key}: {delta}")


def publish_credit_pair(node):
    node.upload("Uncreditedinterchange", wav_fixture(24000, 23), artist=None)
    node.upload("Namedinterchange", wav_fixture(25000, 29), artist="Unknown Artist")


def inspect_credit_pair(node, archived=False):
    entries = []
    for title in ("Uncreditedinterchange", "Namedinterchange"):
        track_id = wait_for(f"{node.name} received {title}", lambda: node.search(title))
        entries.append(node.call("/tracks/" + track_id, token=""))
    missing, credited = entries
    allowed = (None, "") if archived else (None,)
    require(missing.get("artist_name") in allowed and credited.get("artist_name") == "Unknown Artist"
            and missing["artist_id"] != credited["artist_id"],
            f"{node.name} did not preserve an absent credit and its named homonym")
    return {"uncredited": missing["id"], "credited": credited["id"]}


def discover_and_subscribe(nodes, mode, report, main_title, cancel_title):
    a, b, c = nodes
    measurements = report["measurements"]
    if mode == "baseline":
        start = counters(nodes)
        a.connect(b)
        b.connect(a)
        # Both numeric reverse routes have now been explicitly seeded.
        a.ping(b)
        wait_for("B's local track on A after v1 Ping", lambda: a.search("Firstlocal"))
        wait_for("A's local track on B after v1 Ping", lambda: b.search(main_title))
        delta = difference(start, counters(nodes))
        measurements["v1_bidirectional_ping"] = delta
        require(all(delta[n]["catalog_entries"] > 0 for n in "AB") and no_audio(delta),
                "v1 Ping did not eagerly sync metadata in both directions")
        b.connect(c)
        c.connect(b)
        return

    # A and B negotiate soundtime/control/2. C offers only soundtime/p2p/1.
    start = counters(nodes)
    a.connect(b, follow=False)
    b.connect(a, follow=False)
    quiet_catalogue(nodes, start, measurements, "presence_only")
    require(not a.search("Firstlocal") and not b.search(main_title),
            "An unfollowed catalogue became visible after presence")
    b.subscribe(a, True)
    wait_for("the newly followed A catalogue on B", lambda: b.search(main_title))
    delta = difference(start, counters(nodes))
    measurements["one_way_explicit_subscription"] = delta
    require(delta["B"]["catalog_entries"] > 0 and delta["A"]["catalog_entries"] == 0
            and no_audio(delta) and not a.search("Firstlocal"), "Following was not one-way")

    b.subscribe(a, False)
    start = counters(nodes)
    # An authenticated PresenceReply also carries the latest remote subscription.
    b.connect(a, follow=False)
    new_title = "Unfollowprobe"
    a.upload(new_title, wav_fixture(16000, 13))
    quiet_catalogue(nodes, start, measurements, "after_explicit_unfollow")
    require(not b.search(new_title), "Unfollowed publication still entered B's index")
    b.subscribe(a, True)
    wait_for("A's new publication after refollow", lambda: b.search(new_title))
    a.subscribe(b, True)
    wait_for("B's catalogue after reciprocal subscription", lambda: a.search("Firstlocal"))

    start = counters(nodes)
    if mode in ("acceptance", "relay"):
        b.connect(c, follow=False)
        c.connect(b, follow=False)
        quiet_catalogue(nodes, start, measurements, "third_modern_presence")
        b.subscribe(c, True)
        c.subscribe(b, True)
        wait_for("C's catalogue after following on B", lambda: b.search(cancel_title))
        wait_for("B's catalogue after following on C", lambda: c.search("Firstlocal"))
        return
    refusal = b.add_peer(c, follow=False)
    require("legacy peer requires explicit follow" in refusal["message"],
            f"Presence-only contacted a legacy peer unexpectedly: {refusal}")
    c.connect(b)
    quiet_catalogue(nodes, start, measurements, "mixed_before_follow")
    require(not c.search("Firstlocal") and not b.search(cancel_title),
            "Legacy presence crossed the local subscription boundary")
    b.connect(c)  # Manual add with its default follows the catalogue.
    wait_for("legacy C's catalogue on B", lambda: b.search(cancel_title))
    c.connect(b)
    wait_for("B's catalogue on legacy C", lambda: c.search("Firstlocal"))
    delta = difference(start, counters(nodes))
    measurements["mixed_after_follow"] = delta
    require(all(delta[n]["catalog_entries"] > 0 for n in "BC") and no_audio(delta),
            "Explicit follow did not enable the compatible v1 exchanges")


def fetch_range(node, track_id, audio, start, length):
    end = start + length - 1
    with node.open(f"/tracks/{track_id}/stream", headers={"Range": f"bytes={start}-{end}"}) as response:
        require(response.status == 206, f"{node.name}: expected HTTP 206, got {response.status}")
        require(response.headers.get("Content-Range") == f"bytes {start}-{end}/{len(audio)}",
                f"{node.name}: invalid Content-Range")
        require(response.read(length + 1) == audio[start:end + 1], f"{node.name}: incorrect audio bytes")
    return length


def proxy_range_contract(node, track_id, audio, observe_open=None):
    path = f"/tracks/{track_id}/stream"
    size = len(audio)
    for value, start in (("bytes=-1537", size - 1537), (f"bytes={size-811}-", size-811)):
        with node.open(path, headers={"Range": value}) as response:
            require(response.status == 206 and response.headers.get("Accept-Ranges") == "bytes",
                    "Missing proxy range support")
            require(response.headers.get("Content-Range") == f"bytes {start}-{size-1}/{size}" and
                    response.headers.get("Content-Length") == str(size-start), "Incorrect proxy range headers")
            first = response.read(1)
            require(first == audio[start:start + 1], "Incorrect first byte of authenticated suffix")
            if observe_open and value.startswith("bytes=-"):
                observe_open()
            require(first + response.read() == audio[start:], "Incorrect authenticated suffix")
    try:
        with node.open(path, headers={"Range": f"bytes={size}-"}):
            raise ScenarioError("Out-of-bounds proxy request succeeded")
    except HTTPError as exc:
        with exc:
            require(exc.code == 416 and exc.headers.get("Content-Range") == f"bytes */{size}",
                    "Incorrect unsatisfiable proxy response")


def probe_stream(node, track_id, audio):
    try:
        fetch_range(node, track_id, audio, 0, 64)
        return 206
    except HTTPError as exc:
        with exc:
            if exc.code not in (403, 404, 410, 503):
                raise ScenarioError(f"{node.name}: unexpected stream status {exc.code}") from exc
            return exc.code


def chromium_command(node, script, *args):
    mode = os.getenv("FEDERATION_BROWSER", "native")
    if mode == "native":
        return ["node", str(Path(__file__).with_name(script)), node.url, *args]
    project = os.getenv("SOUNDTIME_TEST_COMPOSE_PROJECT", "")
    require(mode == "container" and re.fullmatch(r"st-fed-[0-9a-f]{16}", project)
            and node.name in {"A", "B", "C"}, "Container browser needs this disposable Compose project")
    return ["docker", "compose", "--env-file", "/dev/null", "--project-name", project,
            "--file", str(Path(__file__).with_name("compose.yml")),
            "--file", str(Path(__file__).with_name("browser.yml")),
            "run", "--no-deps", "--rm", "-T", "--entrypoint", "node", "browser",
            "/opt/soundtime-browser/" + script, "http://proxy-" + node.name.lower(), *args]


def browser_command(node, playlist, track, title):
    return chromium_command(node, "browser-playback.mjs", playlist, track, title, "--session")


def write_screenshot(path, encoded, dimensions):
    png = base64.b64decode(encoded, validate=True)
    actual = (int.from_bytes(png[16:20], "big"), int.from_bytes(png[20:24], "big"))
    require(png.startswith(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR") and actual == dimensions,
            f"Invalid PNG or viewport for {path.name}")
    path.write_bytes(png)


def webradio_browser(node, directory):
    command = chromium_command(node, "browser-webradios.mjs", "--pipe")
    # The setup admin's tokens never appear in arguments, artifacts or logs.
    credentials = {"access_token": node.token, "refresh_token": node.refresh_token}
    folder = directory / "screenshots"
    folder.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=directory, prefix="radio-browser-") as profile_dir:
        proc = subprocess.run(command, input=json.dumps(credentials), capture_output=True, text=True,
                              timeout=210, env={**os.environ, "TMPDIR": profile_dir})
    require(proc.stdout.strip(), f"Webradio browser did not produce evidence: {proc.stderr[-900:]}")
    try:
        evidence = json.loads(proc.stdout)
    except json.JSONDecodeError as error:
        raise ScenarioError(f"Invalid web radio browser output at character {error.pos} "
                            f"of {len(proc.stdout)} (exit {proc.returncode}): {proc.stderr[-900:]}") from error
    result = evidence["report"]
    expected = {"webradios-desktop.png": (1366, 900), "webradios-mobile.png": (390, 844),
                "webradios-grid-mobile.png": (390, 844), "webradios-podcast.png": (1366, 900), "webradios-live.png": (1366, 900), "webradios-disabled.png": (1366, 900)}
    for name, image in evidence["screenshots"].items():
        require(name in expected, "Unexpected webradio screenshot")
        write_screenshot(folder / name, image, expected[name])
    (directory / "webradios-browser.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    require(proc.returncode == 0, f"Webradio browser failed: {result.get('error', proc.stderr[-800:])}")
    require(result.get("settings_restored") and set(evidence["screenshots"]) == set(expected),
            "Incomplete webradio browser evidence")
    return result


def save_browser_screenshots(directory, phase, images):
    expected = {"add": {"search-desktop": (1440, 900)},
                "play": {"playlist-desktop": (1440, 900), "player-mobile": (390, 844)}}[phase]
    require(isinstance(images, dict) and images.keys() == expected.keys(), f"Missing browser captures during {phase}")
    directory.mkdir(parents=True, exist_ok=True)
    artifacts = []
    for name, (width, height) in expected.items():
        dest = directory / (name + ".png")
        write_screenshot(dest, images[name], (width, height))
        artifacts.append({"file": f"{directory.name}/{dest.name}", "width": width, "height": height})
    return artifacts


class BrowserSession:
    """Log into the actual UI with fixture credentials sent over the private stdin pipe."""

    def __init__(self, stack, node, track, title, playlists, listeners):
        self.listeners = [{"name": f"Listener {i}", "playlist": pid, **node.login_credentials[account[0]]}
                          for i, (pid, account) in enumerate(zip(playlists, listeners), 1)]
        self.errors = stack.enter_context(tempfile.TemporaryFile(mode="w+", dir=os.getenv("FEDERATION_ARTIFACT_DIR")))
        self.proc = subprocess.Popen(
            browser_command(node, playlists[0], track, title),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.errors, text=True, bufsize=0,
        )
        stack.push(self._exit)

    def phase(self, name, screenshot_dir=None, credit=None):
        require(self.proc is not None, f"Browser session already closed during {name}")
        try:
            self.proc.stdin.write(json.dumps({"phase": name, "listeners": self.listeners,
                                               "capture": screenshot_dir is not None, "credit": credit}) + "\n")
            self.proc.stdin.flush()
        except BrokenPipeError as error:
            raise self.failure(name, "input pipe closed") from error
        if not select.select([self.proc.stdout], [], [], 120)[0]:
            raise ScenarioError(f"Browser timed out during {name}")
        line = self.proc.stdout.readline()
        if not line:
            raise self.failure(name, "output pipe closed")
        result = json.loads(line)
        require(isinstance(result, dict) and result, f"Empty browser measurement during {name}")
        if screenshot_dir is not None:
            result["screenshots"] = save_browser_screenshots(screenshot_dir, name, result.pop("screenshots", None))
        return result

    def failure(self, name, reason):
        fd = self.errors.fileno()
        size = os.fstat(fd).st_size
        if size <= 2000:
            excerpt = os.pread(fd, size, 0)
        else:
            excerpt = os.pread(fd, 1200, 0) + b"\n...\n" + os.pread(fd, 795, size - 795)
        detail = excerpt.decode(errors="replace").strip()
        return ScenarioError(f"Browser {name} failed ({reason})" + (f": {detail}" if detail else ""))

    def _exit(self, _type, original, _traceback):
        try:
            self.close()
        except (OSError, subprocess.SubprocessError) as error:
            if original is None:
                raise
            if hasattr(original, "add_note"):
                original.add_note(f"Browser cleanup also failed: {error}")

    def close(self):
        proc, self.proc = self.proc, None
        if proc is None:
            return
        try:
            with suppress(OSError):
                proc.stdin.close()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                with suppress(ProcessLookupError):
                    proc.terminate()
                try:
                    proc.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    with suppress(ProcessLookupError):
                        proc.kill()
                    proc.wait(timeout=2)
        finally:
            with suppress(OSError):
                proc.stdout.close()


def switch_backend(name, running):
    """Operate only disposable audio backends owned by this scenario."""
    require(name in {"A", "B", "C"}, "Invalid disposable backend")
    action = "start" if running else "stop"
    project = os.getenv("SOUNDTIME_TEST_COMPOSE_PROJECT", "")
    native_driver = os.getenv("SOUNDTIME_TEST_FAULT_DRIVER")
    if re.fullmatch(r"st-fed-[0-9a-f]{16}", project):
        command = ["docker", "compose", "--env-file", "/dev/null", "--project-name", project,
                   "--file", str(Path(__file__).with_name("compose.yml"))]
        if os.getenv("SOUNDTIME_TEST_RELAY") == "1":
            command += ["--file", str(Path(__file__).with_name("relay.yml"))]
        elif os.getenv("SOUNDTIME_TEST_RETENTION") == "1":
            command += ["--file", str(Path(__file__).with_name("retention.yml"))]
        command.append(action)
        if not running:
            command += ["--timeout", "3"]
        command.append("backend-" + name.lower())
    elif native_driver:
        command = [native_driver, action, name]
    else:
        raise ScenarioError("Acceptance needs the disposable Compose project or a native fault driver")
    attempt = subprocess.run(command, capture_output=True, text=True, timeout=45)
    require(attempt.returncode == 0, f"Failed to {action} test backend {name}: {attempt.stderr[-400:]}")


def switch_origin(running):
    switch_backend("A", running)


def origin_failure_cycle(nodes, tracks, audio, playlists, measurements):
    a, b, c = nodes
    remote_id = tracks["from_A_on_B"]
    unrelated_id = tracks["independent_on_B"]
    require(unrelated_id and unrelated_id != remote_id, "Missing independent publication on B")
    identity = a.node_id

    def saved(expected):
        return all(any(row["track_id"] == remote_id and row["position"] == 1
                       and row["availability"] == expected for row in b.positions(pid)) for pid in playlists)

    def origin_ready():
        try:
            a.identify()
            return True
        except (ScenarioError, OSError):
            return False

    block = b.call("/admin/blocked-domains", method="POST", body={"domain": identity, "reason": "Federation fixture"})
    try:
        wait_for("banned origin removed from B's playable catalogue",
                 lambda: not b.search("Fedaudio") and saved("unavailable_on_instance"))
        before = counters(nodes)
        status = probe_stream(b, remote_id, audio)
        denied = difference(before, counters(nodes))
        require(status in (403, 503) and no_audio(denied),
                f"B read a banned origin or substituted another publication: {status}, {denied}")
        before = counters(nodes)
        fetch_range(b, unrelated_id, audio, 3 * 1024 * 1024, 65536)
        independent = difference(before, counters(nodes))
        require(independent["C"]["sent"] >= 65536 and independent["A"]["sent"] == 0,
                "The independent publication did not stream from C while A was banned")
        measurements["peer_ban"] = {"http": status, "denied": denied, "independent_from_C": independent}
    finally:
        b.call("/admin/blocked-domains/" + str(UUID(block["id"])), method="DELETE")

    b.connect(a, follow=True)
    wait_for("same UUID restored after lifting ban", lambda: b.search("Fedaudio") == remote_id and saved("playable"))
    before = counters(nodes)
    fetch_range(b, remote_id, audio, 2 * 1024 * 1024, 65536)
    after = difference(before, counters(nodes))
    require(after["A"]["sent"] >= 65536 and after["C"]["sent"] == 0, "Unblocked publication did not return to A")
    measurements["peer_unban"] = after

    try:
        switch_origin(False)
        # Actively confirm the failed authenticated connection. This doesn't unfollow
        # the catalogue, allowing its UUID to be restored after the origin returns.
        outcome = b.add_peer(a, follow=True)
        require("ping failed" in outcome["message"], f"B still reached the stopped A: {outcome}")
        status = probe_stream(b, remote_id, audio)
        require(status == 503, f"A failed temporarily, expected a retryable 503, got {status}")
        wait_for("confirmed outage reflected in saved playlists",
                 lambda: not b.search("Fedaudio") and saved("temporarily_unavailable"))
        require(b.search("Independentcopy") == unrelated_id, "C's independent title disappeared with A")
        measurements["origin_outage"] = {"http": status, "saved_track": remote_id, "source_node": identity}
    finally:
        switch_origin(True)

    wait_for("restarted A API and P2P identity", origin_ready, timeout=60)
    require(a.node_id == identity, "Restart changed the publisher's cryptographic identity")
    a.connect(b, follow=True)
    b.connect(a, follow=True)
    a.connect(c, follow=True)
    c.connect(a, follow=True)
    wait_for("restored original after peer restart",
             lambda: b.search("Fedaudio") == remote_id and saved("playable"))
    before = counters(nodes)
    handshakes = b.telemetry()["range_connections_established"]
    fetch_range(b, remote_id, audio, 3 * 1024 * 1024 + 65536, 65536)
    resumed = difference(before, counters(nodes))
    require(resumed["A"]["sent"] >= 65536 and resumed["C"]["sent"] == 0, "Restarted A did not serve its publication")
    reconnected = b.telemetry()["range_connections_established"]
    require(reconnected > handshakes, "No fresh authenticated media session after A restarted")
    # The next distinct window must use the recovered session of the same supplier.
    fetch_range(b, remote_id, audio, 3 * 1024 * 1024 + 131072, 65536)
    require(b.telemetry()["range_connections_established"] == reconnected, "Recovered session was not reused")
    measurements["origin_reconnected"] = {"identity": a.node_id, "saved_track": remote_id, "p2p": resumed,
                                          "media_handshakes": reconnected - handshakes, "warm_session_reused": True}


def check(report, name, passed, milestone=1):
    report["checks"][name] = {"passed": bool(passed), "enforced": report["mode"] in ("acceptance","relay"), "task": milestone}


def exercise(args, report):
    with ExitStack() as stack:
        _exercise(args, report, stack)


def _exercise(args, report, stack):
    a, b, c = nodes = [Instance(k, getattr(args, k.lower())) for k in "ABC"]
    require(len({n.url for n in nodes}) == 3, "Three distinct base URLs are required")
    for n in nodes:
        status = n.call("/setup/status")
        require(not status["has_admin"] and not status["setup_complete"],
                f"{n.name}: a disposable empty instance is required")

    accounts = report["accounts"] = {}
    admins = {}
    for n in nodes:
        admins[n.name] = n.account("admin" + n.name.lower(), admin=True)
        accounts["admin_" + n.name] = admins[n.name][0]
        n.identify()
    report["nodes"] = {n.name: {"node_id": n.node_id, "direct_addresses": n.addresses} for n in nodes}
    modes = {n.name: n.telemetry()["explicit_catalog"] for n in nodes}
    report["explicit_catalog"] = modes
    report["relay_only"] = {n.name: n.relay_only for n in nodes}
    require(all(n.relay_only == (args.mode == "relay") for n in nodes), "Unexpected physical P2P topology")
    pilot = args.mode in ("acceptance","relay")
    require(modes == {"A": args.mode != "baseline", "B": args.mode != "baseline", "C": pilot},
            f"Unexpected wire modes for {args.mode}: {modes}")
    require(len({n.node_id for n in nodes}) == 3, "Duplicate P2P identity")
    listeners = [b.account("listenerone"), b.account("listenertwo")]
    accounts["listeners_B"] = [account_id for account_id, _ in listeners]
    require(len(set(accounts["listeners_B"])) == 2, "Duplicate listener identity")

    main_audio = wav_fixture(4 * 1024 * 1024, 7)
    cancel_audio = wav_fixture(8 * 1024 * 1024, 11)
    main_title, other_title, cancel_title = "Fedaudio", "Independentcopy", "Cancellationaudio"
    tracks = report["tracks"] = {}
    credit = {"artist_name": "SoundTime fixture", "album_title": "Federation sessions"} if args.mode == "acceptance" else None
    tracks["origin_A"] = a.upload(main_title, main_audio, album=credit["album_title"] if credit else None)
    tracks["cancel_C"] = c.upload(cancel_title, cancel_audio)
    if pilot:
        cancel_a_audio = wav_fixture(8 * 1024 * 1024, 17)
        tracks["cancel_A"] = a.upload("CancelatA", cancel_a_audio)
    if args.mode == "compatibility":
        publish_credit_pair(b)
    anchors = [b.upload("Firstlocal", wav_fixture(16000, 3)),
               b.upload("Lastlocal", wav_fixture(16000, 5))]
    tracks["anchors_B"] = anchors
    report["fixtures"] = {k: {"bytes": len(audio), "sha256": hashlib.sha256(audio).hexdigest()}
                          for k, audio in ((main_title, main_audio), (cancel_title, cancel_audio))}

    origin_list = a.playlist(admins["A"], "Origin playlist", [tracks["origin_A"]])
    measurements = report["measurements"] = {}
    start = counters(nodes)
    discover_and_subscribe(nodes, args.mode, report, main_title, cancel_title)
    if args.mode == "compatibility":
        measurements["credit_interop"] = {n.name: inspect_credit_pair(n) for n in (a, b, c)}
        return
    tracks["from_A_on_B"] = wait_for("A's publication in B's local index", lambda: b.search(main_title))
    # Establish B's reference to A before introducing another possible provider.
    tracks["from_C_on_B"] = wait_for("C's publication in B's local index", lambda: b.search(cancel_title))
    require(tracks["from_A_on_B"] != tracks["from_C_on_B"], "Distinct audio resolved to one track")
    after_sync = counters(nodes)
    measurements["discovery_and_catalog"] = difference(start, after_sync)
    require(b.search(main_title) == tracks["from_A_on_B"], "Unstable UUID returned by local search")
    order = [anchors[0], tracks["from_A_on_B"], anchors[1]]
    initial_order = [anchors[0]] if pilot else order
    playlists = [b.playlist(user, f"Listener {i}", initial_order) for i, user in enumerate(listeners, 1)]
    require(len(set(playlists)) == 2, "Two separate playlists are required")
    browser = BrowserSession(stack, b, tracks["from_A_on_B"], main_title, playlists, listeners)
    screenshot_dir = args.report.parent / "screenshots" if args.mode == "acceptance" else None
    if pilot:
        measurements["browser_playlist_additions"] = browser.phase("add", screenshot_dir, credit)
        require(measurements["browser_playlist_additions"]["added"] == playlists,
                "The browser did not add both listeners' memberships")
        for pid, (_, token) in zip(playlists, listeners):
            b.call(f"/playlists/{pid}/tracks", method="POST", token=token,
                   body={"track_id": anchors[1], "position": 2})
    before = {pid: b.positions(pid) for pid in playlists}
    require(all([(t["position"], t["track_id"]) for t in rows] == list(enumerate(order))
                for rows in before.values()),
            "Playlist does not contain the three expected tracks in order")
    if credit:
        hits = b.call("/search?" + urlencode({"q": main_title, "include_p2p": "false"}))["tracks"]
        searched = [row for row in hits if row["id"] == tracks["from_A_on_B"]]
        require(len(searched) == 1 and all(searched[0].get(k) == v for k, v in credit.items()),
                "Search lost the remote artist or album credit")
        for rows in before.values():
            saved = rows[1]
            require(saved["track_id"] == searched[0]["id"] and saved["position"] == 1
                    and saved["availability"] == searched[0]["availability"] == "playable"
                    and all(saved.get(k) == searched[0].get(k) for k in credit),
                    "Playlist credit, position or availability disagrees with the indexed search hit")
        check(report, "remote_credit_matches_search_and_both_playlists", True)
    origin_before = a.positions(origin_list)
    require([t["track_id"] for t in origin_before] == [tracks["origin_A"]], "Origin playlist is incomplete")
    report["playlists"] = {"on_B_before": before, "on_A_before": {origin_list: origin_before}}
    for _, token in listeners:
        b.call("/favorites/" + tracks["from_A_on_B"], method="POST", token=token)
        b.call("/tracks/" + tracks["from_A_on_B"], token=token)
    if pilot:
        observations = measurements["zero_cache_reader_volume"] = {}
        signatures = []
        for audio in (main_audio, cancel_audio, cancel_a_audio):
            with wave.open(io.BytesIO(audio)) as fixture:
                signatures.append(fixture.readframes(128))
        observe_zero_cache(b, signatures, observations, "before_remote_play")
    before_range = counters(nodes)
    measurements["search_and_playlist"] = difference(after_sync, before_range)
    check(report, "metadata_and_search_transfer_zero_audio",
          all(no_audio(measurements[phase]) for phase in ("discovery_and_catalog", "search_and_playlist")))

    started = time.monotonic()
    delivered = fetch_range(b, tracks["from_A_on_B"], main_audio, 1024 * 1024, 65536)
    elapsed = time.monotonic() - started
    after_range = counters(nodes)
    remote = difference(before_range, after_range)
    measurements["first_remote_range"] = {"http_bytes": delivered, "http_elapsed_s": elapsed,
                                          "p2p": remote}
    require(remote["B"]["received"] >= delivered and remote["A"]["sent"] >= delivered,
            "Cold range did not traverse the expected A to B P2P path")
    if args.mode == "relay":
        require(remote["B"]["relay_windows"] == remote["B"]["verified_windows"] == 1,
                "Cold verified audio did not traverse the isolated relay")
        check(report, "verified_cold_range_on_relay", True, milestone=11)
    check(report, "range_downloads_less_than_the_entire_file",
          remote["B"]["received"] < len(main_audio), milestone=9)

    cancel_id = tracks["from_C_on_B"]
    if pilot:
        require(remote["B"]["verified_windows"] == 1 and remote["B"]["proof_bytes"] > 0 and
                remote["B"]["received"] == delivered, "Seek did not use one verified Bao window")
        proxy_range_contract(b, tracks["from_A_on_B"], main_audio,
                             lambda: observe_zero_cache(b, signatures, observations, "with_range_reader_open"))
        try:
            with b.open("/admin/cache/tracks/" + tracks["from_A_on_B"] + "/pin", method="PUT"):
                raise ScenarioError("Zero quota admitted a pin")
        except HTTPError as exc:
            with exc:
                require(exc.code == 507, f"Zero quota pin returned HTTP {exc.code}")
        cancel_id = wait_for("v2 cancellation fixture on B", lambda: b.search("CancelatA"))
        cancel_audio = cancel_a_audio
    before_cancel = counters(nodes)
    # A second cold file makes cancellation measurable independently of the first range.
    started = time.monotonic()
    with b.open(f"/tracks/{cancel_id}/stream") as response:
        require(response.status == 200, "Expected full stream HTTP 200")
        prefix = response.read(16384)
        require(prefix == cancel_audio[:16384], "Cancelled stream delivered incorrect bytes")
        first_chunk_s = time.monotonic() - started
    immediately_after_close = counters(nodes)
    window = 2
    time.sleep(window)
    after_window = counters(nodes)
    cancel = difference(before_cancel, after_window)
    measurements["cancel_remote_stream"] = {
        "http_bytes_read": len(prefix), "first_chunk_s": first_chunk_s,
        "p2p_total": cancel, "p2p_delta_after_first_post_close_sample":
        difference(immediately_after_close, after_window), "observation_window_s": window,
    }
    cancel_origin = "A" if pilot else "C"
    require(cancel[cancel_origin]["sent"] >= len(prefix) and cancel["B"]["received"] >= len(prefix),
            f"Cancellation fixture did not traverse {cancel_origin} to B")
    if pilot:
        require(0 < cancel["B"]["verified_windows"] and cancel["B"]["received"] < len(cancel_audio),
                "Cancelling the browser-side connection fetched the entire cold publication")
        observe_zero_cache(b, signatures, observations, "after_reader_close")
        observations["transport"] = {"range_audio_bytes_received": remote["B"]["received"],
                                     "range_proof_bytes_received": remote["B"]["proof_bytes"],
                                     "cancel_audio_bytes_received": cancel["B"]["received"],
                                     "cancel_proof_bytes_received": cancel["B"]["proof_bytes"]}

    # C receives and verifies a full copy of A before creating its own independent
    # publication. This transfer is explicit and outside B's cold-read measurements.
    a.connect(c)
    c.connect(a)
    tracks["from_A_on_C"] = wait_for("A's publication in C's index", lambda: c.search(main_title))
    preload_start = counters(nodes)
    fetch_range(c, tracks["from_A_on_C"], main_audio, 0, len(main_audio))
    preload = difference(preload_start, counters(nodes))
    require(preload["C"]["received"] >= len(main_audio), "C did not retrieve a complete remote copy")
    measurements["preload_copy_on_C"] = {"verified_bytes": len(main_audio), "p2p": preload}
    tracks["independent_C"] = c.upload(other_title, main_audio)
    require(tracks["independent_C"] != tracks["from_A_on_C"], "C overwrote A's publication on upload")
    # A ping's response can precede asynchronous catalog import. Observe briefly even
    # on the legacy protocol, whose hash deduplication discards the second publication.
    c.connect(b)
    deadline = time.monotonic() + 4
    while True:
        independent = b.search(other_title)
        if independent or time.monotonic() >= deadline:
            break
        time.sleep(0.25)
    tracks["independent_on_B"] = independent
    check(report, "independent_publication_indexed_separately",
          independent not in (None, tracks["from_A_on_B"]), milestone=6)
    if pilot:
        origin_failure_cycle(nodes, tracks, main_audio, playlists, measurements)
        check(report, "ban_outage_and_authenticated_recovery", True, milestone=11)
        before_browser = counters([b])
    measurements["browser_playback"] = browser.phase("play", screenshot_dir, credit)
    if pilot:
        # The play phase has decoded, sought and navigated away from the media page.
        played = difference(before_browser, counters([b]))["B"]
        require(played["received"] > 0 and played["verified_windows"] > 0,
                "Chromium playback did not receive verified remote audio on B")
        observations["transport"].update(browser_audio_bytes_received=played["received"],
                                         browser_proof_bytes_received=played["proof_bytes"])
        observe_zero_cache(b, signatures, observations, "after_browser_player_close")
        check(report, "zero_cache_stream_leaves_no_remote_audio_or_partial_files", True, milestone=11)
    for _, token in listeners:
        b.call("/history", method="POST", token=token,
               body={"track_id": tracks["from_A_on_B"], "duration_listened": 4})
    a.call("/tracks/" + tracks["origin_A"], method="DELETE")
    origin_after = a.positions(origin_list)
    report["playlists"]["on_A_after"] = {origin_list: origin_after}
    check(report, "origin_withdrawal_preserves_playlist_position",
          [(t["position"], t["track_id"]) for t in origin_after] == [(0, tracks["origin_A"])], milestone=3)
    # A timeout, 404, or a legacy 410 on its own cannot establish that an
    # authenticated withdrawal was applied. Also observe the per-publication state.
    started = time.monotonic()
    remote_status = probe_stream(b, tracks["from_A_on_B"], main_audio)
    while remote_status != 410 and time.monotonic() - started < 5:
        time.sleep(0.5)
        remote_status = probe_stream(b, tracks["from_A_on_B"], main_audio)
    report["withdrawal"] = {"on_B_http_status": remote_status, "observation_s": time.monotonic() - started,
                            "on_A_http_status": probe_stream(a, tracks["origin_A"], main_audio),
                            "on_C_copy_http_status": probe_stream(c, tracks["from_A_on_C"], main_audio)}
    after = {pid: b.positions(pid) for pid in playlists}
    report["playlists"]["on_B_after"] = after
    withdrawn = all(any(t["track_id"] == tracks["from_A_on_B"] and t["availability"] == "withdrawn"
                        for t in rows) for rows in after.values())
    check(report, "remote_withdrawal_rejects_new_range", remote_status == 410 and withdrawn, milestone=6)
    check(report, "both_listeners_keep_order_and_see_withdrawal",
          all([(t["position"], t["track_id"]) for t in rows] == list(enumerate(order))
              and rows[1]["availability"] == "withdrawn"
              for rows in after.values()), milestone=8)
    for i, (_, token) in enumerate(listeners, 1):
        favorites = b.call("/favorites", token=token)["data"]
        history = [entry["track"] for entry in b.call("/history", token=token)["data"]]
        for name, saved in (("favorites", favorites), ("history", history)):
            if credit:
                retained = [t for t in saved if t["id"] == tracks["from_A_on_B"]]
                require(retained and all(t.get(k) == v for t in retained for k, v in credit.items()),
                        f"Listener {i} lost the saved recording credit in {name} after withdrawal")
            check(report, f"listener_{i}_{name}_retains_withdrawn_track",
                  any(t["id"] == tracks["from_A_on_B"] and t.get("availability") == "withdrawn"
                      for t in saved), milestone=8)
    if pilot:
        measurements["browser_withdrawal"] = browser.phase("withdraw")
        require(measurements["browser_withdrawal"]["disabled"] == playlists,
                "The browser did not retain and disable both withdrawn entries")
        check(report, "browser_playlist_actions_and_withdrawal", True, milestone=8)
    fetch_range(c, tracks["independent_C"], main_audio, 0, 64)
    check(report, "independent_publication_still_plays", True)
    if args.mode == "acceptance":
        from account_withdrawal import exercise as withdraw_creator
        withdraw_creator(a, b, listeners[0], tracks["cancel_A"], anchors[0],
                         report.setdefault("creator_account_withdrawal", {}))
        check(report, "creator_account_withdrawal_survives_both_restarts", True, milestone=3)
        browser.close()
        measurements["webradios_browser"] = webradio_browser(b, args.report.parent)
        check(report, "webradios_browser_and_permissions", True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("a", "b", "c"):
        parser.add_argument("--" + option, required=True)
    parser.add_argument("--mode", required=True, choices=("baseline", "compatibility", "acceptance", "relay"))
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    report = {"schema": 1, "mode": args.mode, "started_at": datetime.now(timezone.utc).isoformat(),
              "checks": {}, "status": "failed"}
    try:
        root = Path(__file__).resolve().parents[2]
        report["revision"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
        report["worktree_dirty"] = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=root))
        exercise(args, report)
        failed = [k for k, v in report["checks"].items() if v["enforced"] and not v["passed"]]
        if args.mode in ("acceptance","relay") and failed:
            raise ScenarioError("Failed acceptance checks: " + ", ".join(failed))
        report["status"] = "observed" if args.mode == "baseline" else ("compatible" if args.mode == "compatibility" else "accepted")
    except Exception as exc:
        report["error"] = f"{type(exc).__name__}: {exc}"
        print(report["error"], file=sys.stderr)
    finally:
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return int(report["status"] == "failed")


if __name__ == "__main__":
    sys.exit(main())
