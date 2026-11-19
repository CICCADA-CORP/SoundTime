import assert from "node:assert/strict";
import test from "node:test";
import { AudioDelivery } from "./load-audio.mjs";

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
