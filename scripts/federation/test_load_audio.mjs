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
