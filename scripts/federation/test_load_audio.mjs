import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import http from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PassThrough } from "node:stream";
import test from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { AudioDelivery, MediaRead } from "./load-audio.mjs";

test("a buffered listener, an empty socket and an interrupted listener", () => {
  const audio = new AudioDelivery(3, 1000, 4000, 1000);
  audio.record(0, 500, 1000);
  audio.record(0, 1500, 1000);
  audio.record(0, 2500, 1000);
  audio.record(0, 3500, 1000);
  audio.record(2, 2000, 500);
  audio.record(2, 4100, 1000);
  audio.failed(2, false);
  audio.failed(2, true);
  const [healthy, empty, interrupted] = audio.results();
  assert.equal(healthy.bytes_per_second, 1000);
  assert.equal(healthy.longest_unbuffered_ms, 0);
  assert.equal(empty.bytes_per_second, 0);
  assert.equal(empty.longest_unbuffered_ms, 3000);
  assert.equal(interrupted.longest_unbuffered_ms, 1500);
  assert.equal(interrupted.bytes, 500);
  assert.equal(interrupted.failed_starts, 1);
  assert.equal(interrupted.interruptions, 1);
});

test("a paced HTTP reader keeps buffered bytes past its socket timeout", {skip: process.platform === "win32"}, async t => {
  const folder = await mkdtemp(join(tmpdir(), "st-audio-"));
  const socketPath = join(folder, "test.sock");
  const total = 512 * 1024;
  const server = http.createServer((request, response) => {
    if (request.url === "/body") response.end(Buffer.alloc(total, 3));
  });
  t.after(async () => {
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
    await rm(folder, {recursive: true, force: true});
  });
  server.listen(socketPath);
  await once(server, "listening");
  const read = new MediaRead(100);
  const req = http.get({socketPath, path:"/body", agent:false});
  read.watchHeaders(req);
  const [res] = await once(req, "response");
  let bytes = 0;
  for await (const chunk of read.chunks(res)) {
    if (!bytes) {
      read.pace(300);
      await delay(300);
      assert.ok(res.readableLength > 0);
    }
    bytes += chunk.length;
  }
  assert.equal(bytes, total);
  assert.equal(read.failure, null);
  const pending = new MediaRead(50);
  const stalled = http.get({socketPath, path:"/no-headers", agent:false});
  pending.watchHeaders(stalled);
  const [error] = await once(stalled, "error");
  assert.match(error.message, /inactivity_timeout/);
  assert.equal(pending.failure.cause, "local_header_timeout");
});

test("a real wait with no body bytes after a chunk still expires", async () => {
  const read = new MediaRead(40);
  const response = new PassThrough();
  response.write(Buffer.from("ok"));
  const chunks = read.chunks(response);
  assert.equal((await chunks.next()).value.toString(), "ok");
  await assert.rejects(chunks.next(), /inactivity_timeout/);
  assert.equal(read.failure.phase, "await_body");
  assert.equal(read.failure.cause, "local_body_timeout");
  assert.equal(read.failure.buffered_bytes, 0);
  assert.ok(read.failure.since_last_chunk_ms > 0);
});

test("a peer reset during local pacing is recorded before the next read", async () => {
  const body = new PassThrough();
  const read = new MediaRead(40);
  body.write(Buffer.from("ok"));
  const chunks = read.chunks(body);
  await chunks.next();
  read.pace(80);
  body.destroy(Object.assign(new Error("remote reset"), {code: "ECONNRESET"}));
  await once(body, "error");
  await assert.rejects(chunks.next(), {code:"ECONNRESET"});
  assert.equal(read.failure.cause, "response_error");
  assert.equal(read.failure.phase, "paced_pause");
  assert.equal(read.failure.requested_pace_ms, 80);
});

test("retry delays grow, stay bounded and stagger peers after a shared failure", async () => {
  const { audioRetryDelayMs } = await import("./load-audio.mjs");
  for (const listener of [0, 32, 999]) {
    const first = audioRetryDelayMs(listener, 1);
    assert.ok(first >= 200 && first <= 250);
    assert.ok(audioRetryDelayMs(listener, 3) > audioRetryDelayMs(listener, 2));
    assert.ok(audioRetryDelayMs(listener, 10_000) <= 5000);
  }
  assert.notEqual(audioRetryDelayMs(0, 6), audioRetryDelayMs(1, 6));
});

test("a reset resumes at the next verified byte, including a failed retry at a nonzero offset", async () => {
  const { AudioCursor } = await import("./load-audio.mjs");
  const seed = 3, start = 70, size = 10000;
  const audio = Buffer.from(Array.from({length: size}, (_, i) => ((i - 44) % 256) * seed % 127 + 64));
  const cursor = new AudioCursor(size, seed, start);
  assert.equal(cursor.beginRequest(), start);
  const interruptedBody = async function* () {
    yield audio.subarray(start, 4321);
    throw Object.assign(new Error("reset"), {code: "ECONNRESET"});
  };
  await assert.rejects(async () => {
    for await (const chunk of interruptedBody()) cursor.accept(chunk);
  }, {code: "ECONNRESET"});
  assert.equal(cursor.beginRequest(), 4321);
  // No bytes from a failed start; a damaged chunk also cannot advance the cursor.
  assert.throws(() => cursor.accept(Buffer.from([0])), /incorrect_audio/);
  assert.equal(cursor.beginRequest(), 4321);
  cursor.accept(audio.subarray(4321));
  assert.equal(cursor.offset, size);
  assert.equal(cursor.beginRequest(), 0);
});
