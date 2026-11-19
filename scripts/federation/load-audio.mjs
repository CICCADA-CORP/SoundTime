// Count verified body bytes per listener and detect gaps after its received buffer
// would have emptied at the requested playback rate. Include stalls at the deadline.
export class AudioDelivery {
  constructor(clients, steady, end, rate) {
    this.start = steady;
    this.end = end;
    this.rate = rate;
    this.listeners = Array.from({ length: clients }, () => ({
      bytes: 0, availableUntil: 0, longestGap: 0, failedStarts: 0, interruptions: 0,
    }));
  }

  record(index, now, bytes) {
    if (now > this.end) return;
    const listener = this.listeners[index];
    if (now >= this.start) {
      listener.bytes += bytes;
      listener.longestGap = Math.max(listener.longestGap, now - Math.max(this.start, listener.availableUntil));
    }
    listener.availableUntil = Math.max(now, listener.availableUntil) + bytes * 1000 / this.rate;
  }

  failed(index, receivedBody) {
    if (receivedBody) this.listeners[index].interruptions++;
    else this.listeners[index].failedStarts++;
  }

  results() {
    const seconds = (this.end - this.start) / 1000;
    return this.listeners.map(({ bytes, availableUntil, longestGap, interruptions, failedStarts }, index) => ({
      listener: index, bytes, interruptions, failed_starts: failedStarts,
      bytes_per_second: bytes / seconds,
      longest_unbuffered_ms: Math.max(longestGap, this.end - Math.max(this.start, availableUntil)),
    }));
  }
}

// Desynchronise listeners during a shared outage. A successful verified body chunk resets the streak.
export function audioRetryDelayMs(listener, failures) {
  const base = Math.min(5000, 250 * 2 ** Math.min(5, failures - 1));
  const jitter = (37 * listener + 71 * failures) % 101;
  return Math.round(base * (0.80 + jitter / 500));
}


// Only verified body bytes advance playback. A failed response leaves the
// cursor at the next byte, including when several consecutive starts fail.
export class AudioCursor {
  constructor(size, seed, start) {
    this.size = size;
    this.seed = seed;
    this.offset = start;
  }
  beginRequest() {
    if (this.offset === this.size) this.offset = 0;
    return this.offset;
  }
  accept(chunk) {
    if (this.offset + chunk.length > this.size) throw Error("incorrect_audio");
    for (let j = 0; j < chunk.length; j += 4096) {
      const position = this.offset + j;
      if (position >= 44 && chunk[j] !== ((position - 44) % 256) * this.seed % 127 + 64)
        throw Error("incorrect_audio");
    }
    this.offset += chunk.length;
  }
}

// Capture where a failure was observed, even when Node later reports a generic
// ECONNRESET to its async iterator. A client's pacing sleep is not a network wait.
export class MediaRead {
  constructor(timeoutMs = 15000) {
    this.timeoutMs = timeoutMs;
    this.phase = "headers";
    this.lastByte = null;
    this.failure = null;
    this.requestedPaceMs = 0;
  }

  watchHeaders(request) {
    request.setTimeout(this.timeoutMs, () => {
      const error = new Error("inactivity_timeout");
      this.observe("local_header_timeout", null, error);
      request.destroy(error);
    });
    request.once("response", () => request.setTimeout(0));
  }

  observe(cause, response, error) {
    this.failure ??= {
      cause, phase: this.phase, code: error?.code || error?.message,
      buffered_bytes: response?.readableLength ?? null,
      socket_buffered_bytes: response?.socket?.readableLength ?? null,
      since_last_chunk_ms: this.lastByte === null ? null : Math.round(performance.now() - this.lastByte),
      requested_pace_ms: Math.round(this.requestedPaceMs),
    };
  }

  async *chunks(response) {
    const iterator = response[Symbol.asyncIterator]();
    const onError = error => this.observe("response_error", response, error);
    response.on("error", onError);
    try {
      while (true) {
        this.phase = "await_body";
        this.requestedPaceMs = 0;
        const timeout = setTimeout(() => {
          const error = new Error("inactivity_timeout");
          this.observe("local_body_timeout", response, error);
          response.destroy(error);
        }, this.timeoutMs);
        let next;
        try {
          next = await iterator.next();
        } finally {
          clearTimeout(timeout);
        }
        if (next.done) return;
        this.lastByte = performance.now();
        this.phase = "consume";
        yield next.value;
      }
    } finally {
      response.off("error", onError);
      await iterator.return?.();
    }
  }

  pace(durationMs) {
    this.phase = "paced_pause";
    this.requestedPaceMs = durationMs;
  }
}
