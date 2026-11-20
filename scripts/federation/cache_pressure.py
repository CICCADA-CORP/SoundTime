"""Verify physical free-space admission, competing pins and uncached streaming."""
from concurrent.futures import ThreadPoolExecutor
from threading import Barrier
from urllib.error import HTTPError

from scenario import counters, difference, fetch_range, require, wait_for, wav_fixture


MIB = 1024 * 1024


def cache(node):
    return node.call("/admin/storage/status")["cache"]


def pin_request(node, endpoint):
    try:
        with node.open(endpoint, method="PUT") as response:
            return response.status
    except HTTPError as error:
        with error:
            return error.code


def pressure(origin, reader):
    start = cache(reader)
    require(start["reserve_free_bytes"] == MIB and 9 * MIB <= start["available_bytes"] <= 10 * MIB,
            "Pressure test needs the isolated 10 MiB cache filesystem with its 1 MiB reserve")
    require(start["maximum_bytes"] >= 32 * MIB and not start["pinned_bytes"],
            "Pressure test needs a logical quota larger than the isolated filesystem")
    audio, tracks, pins = [], [], []
    for name, seed in (("Pressureanchor", 3), ("Pressurefirst", 5), ("Pressuresecond", 7)):
        audio.append(wav_fixture(4 * MIB, seed))
        origin.upload(name, audio[-1])
        track = wait_for(name + " indexed on C", lambda: reader.search(name))
        tracks.append(track)
        pins.append("/admin/cache/tracks/" + track + "/pin")
    require(pin_request(reader, pins[0]) == 200, "First physical pin failed")

    gate = Barrier(2)

    def compete(endpoint):
        gate.wait(timeout=10)
        return pin_request(reader, endpoint)

    before = counters([origin, reader])
    with ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(compete, pins[1:]))
    require(sorted(results) == [200, 507], f"Competing cold pins returned {results}")
    admitted = results.index(200) + 1
    rejected = 3 - admitted
    transfers = difference(before, counters([origin, reader]))
    require(transfers[origin.name]["sent"] == transfers[reader.name]["received"] == len(audio[admitted]),
            "Reservation failure downloaded data, or the successful pin fetched duplicate bytes")
    full = cache(reader)
    require(full["reserve_free_bytes"] <= full["available_bytes"] < 2 * MIB
            and full["pinned_bytes"] >= len(audio[0]) + len(audio[admitted]) and full["reserved_bytes"] == 0,
            "Physical space accounting did not protect both pins and the free-space reserve")
    for index, endpoint in enumerate(pins):
        require(reader.call(endpoint)["pinned"] == (index != rejected), "Incorrect durable pin membership")
    try:
        with reader.open("/admin/storage/cache", method="PUT", body={"maximum_bytes": 0}):
            require(False, "The operator could shrink the quota below protected pins")
    except HTTPError as error:
        with error:
            require(error.code == 409, "Quota reduction did not explain its conflict with the pins")
    require(cache(reader)["maximum_bytes"] == start["maximum_bytes"], "Rejected quota change was applied")

    with reader.open("/tracks/" + tracks[rejected] + "/stream") as stream:
        require(stream.status == 200 and stream.read(len(audio[rejected]) + 1) == audio[rejected],
                "A listener could not stream the rejected pin in full")
    streamed = cache(reader)
    for field in ("evictable_bytes", "pinned_bytes", "available_bytes", "reserved_bytes"):
        require(streamed[field] == full[field], "Streaming consumed protected space: " + field)
    fetch_range(reader, tracks[admitted], audio[admitted], 256, 4096)
    # Once an operator releases a pin, its physical blocks can be evicted and reused.
    reader.call(pins[0], method="DELETE")
    require(pin_request(reader, pins[rejected]) == 200, "Released space could not admit the rejected pin")
    require(reader.call(pins[admitted])["pinned"] and reader.call(pins[rejected])["pinned"],
            "Replacing the released pin evicted another protected copy")
    recovered = cache(reader)
    require(recovered["available_bytes"] >= recovered["reserve_free_bytes"] and not recovered["reserved_bytes"],
            "Re-admission consumed the filesystem reserve")
    for index in (admitted, rejected):
        reader.call(pins[index], method="DELETE")
    # Release only this test's temporary pins and cache so the mirror stage really starts cold.
    reader.call("/admin/storage/cache", method="PUT", body={"maximum_bytes": 0})
    reader.call("/admin/storage/cache", method="PUT", body={"maximum_bytes": start["maximum_bytes"]})
    clean = cache(reader)
    require(not any(clean[k] for k in ("pinned_bytes", "evictable_bytes", "reserved_bytes")),
            "Pressure test left cached audio in the mirror scenario")
    return {"before": start, "pin_http": results, "at_capacity": full, "pin_audio": transfers,
            "after_stream": streamed, "after_replacement": recovered, "after_cleanup": clean}
