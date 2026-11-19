// Internal driver. All inputs and regular-listener credentials arrive on a private stdin pipe.
import { execFileSync } from "node:child_process";
import { setMaxListeners } from "node:events";
import http from "node:http";
import { AudioDelivery } from "./load-audio.mjs";
import { monitorEventLoopDelay, performance } from "node:perf_hooks";
import { createInterface } from "node:readline";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const [line] = await Array.fromAsync(createInterface({ input: process.stdin }));
const cfg = JSON.parse(line);
const project = process.env.SOUNDTIME_TEST_COMPOSE_PROJECT || "";
if (!/^st-fed-[0-9a-f]{16}$/.test(project) || !["load-smoke", "load-full"].includes(cfg.profile))
  throw Error("An explicit profile and disposable project are required");
const composeFile = fileURLToPath(new URL("./compose.yml", import.meta.url));
const dockerPort = execFileSync("docker", [
  "compose", "--env-file", "/dev/null", "-p", project, "-f", composeFile, "port", "proxy-b", "80",
], { encoding: "utf8", timeout: 10000 }).trim();
if (!/^127\.0\.0\.1:\d+$/.test(dockerPort) || cfg.base !== "http://" + dockerPort || !cfg.tokens?.length)
  throw Error("Load clients require the current project's local proxy and listener credentials");

const full = cfg.profile === "load-full";
const clients = full ? 1000 : 6;
const rate = full ? 100 : 3;
const duration = full ? 90000 : 15000;
const ramp = full ? 20000 : 2000;
const readRate = 32 * 1024;
const pendingLimit = Math.max(rate * 3, 16);
const agent = new http.Agent({ keepAlive: true, maxSockets: clients + pendingLimit + 10 });
const controller = new AbortController();
setMaxListeners(0, controller.signal);

const loop = monitorEventLoopDelay({ resolution: 10 });
loop.enable();
const start = performance.now();
const steady = start + ramp;
const end = steady + duration;
const audioDelivery = new AudioDelivery(clients, steady, end, readRate);
const startedCpu = process.cpuUsage();
const startedLoop = performance.eventLoopUtilization();
const searchTimes = [], scheduledTimes = [], firstBytes = [], steadyFirstBytes = [];
const dispatchLag = [], samples = [], errors = {};
const pending = new Set();
const mediaBytesByOrigin = {};
let sessions = 0, openResponses = 0, peakOpen = 0, issued = 0, queries = 0, bytes = 0;
let due = 0, searchStarts = 0, omitted = 0, searchErrors = 0;
let mediaAttempts = 0, mediaStarts = 0, failedStarts = 0, interruptions = 0;

function countError(kind, error) {
  const key = kind + "_" + (error.code || error.message);
  errors[key] = (errors[key] || 0) + 1;
}

function get(path, listener, headers = {}) {
  const credential = cfg.tokens[listener % cfg.tokens.length];
  return new Promise((resolve, reject) => {
    const req = http.get(new URL(path, cfg.base), {
      agent, signal: controller.signal, headers: { ...headers, Authorization: "Bearer " + credential },
    }, resolve);
    req.setTimeout(15000, () => req.destroy(Error("inactivity_timeout")));
    req.on("error", reject);
  });
}

async function search(index, scheduled) {
  queries++;
  const begin = performance.now();
  const origin = 1 + index % cfg.virtual_catalogues;
  const term = index % 4 === 0 ? "Melody" + String(Math.floor(index / 4) % 20).padStart(2, "0")
    : "Quartz" + String(origin).padStart(4, "0");
  try {
    const res = await get("/api/search?q=" + term + "&include_p2p=false&limit=10", index);
    let body = "", size = 0;
    for await (const chunk of res) {
      size += chunk.length;
      if (size > 2 * 1024 * 1024) throw Error("response_too_large");
      body += chunk;
    }
    if (res.statusCode !== 200) throw Error("invalid_search_" + res.statusCode);
    if (!JSON.parse(body).tracks?.some(track => track.title?.split(" ").includes(term)))
      throw Error("missing_search_result");
    if (scheduled >= steady && performance.now() <= end) {
      searchTimes.push(performance.now() - begin);
      scheduledTimes.push(performance.now() - scheduled);
    }
  } catch (error) {
    if (!controller.signal.aborted) {
      if (scheduled >= steady) searchErrors++;
      countError("search", error);
    }
  } finally {
    queries--;
  }
}

async function session(index) {
  await delay(index * ramp / clients);
  const group = index % 4;
  // Half request one popular publication. Others round-robin across all test originals on A and C.
  const track = cfg.media[group < 2 ? 0 : (Math.floor(index / 4) * 2 + group - 1) % cfg.media.length];
  let offset = group === 0 ? 0 : 65536 * ((index * 37) % Math.floor(track.size / (2 * 65536)));
  sessions++;
  try {
    while (!controller.signal.aborted) {
      const began = performance.now();
      let counted = false, delivered = 0, res;
      try {
        mediaAttempts++;
        res = await get("/api/tracks/" + track.id + "/stream", index, { Range: "bytes=" + offset + "-" });
        if (res.statusCode !== 206 || res.headers["content-range"] !== "bytes " + offset + "-" + (track.size - 1) + "/" + track.size)
          throw Error("media_status_" + res.statusCode);
        counted = true;
        openResponses++;
        mediaStarts++;
        peakOpen = Math.max(peakOpen, openResponses);
        for await (const chunk of res) {
          const now = performance.now();
          if (!delivered) {
            firstBytes.push(now - began);
            if (began >= steady) steadyFirstBytes.push(now - began);
          }
          for (let j = 0; j < chunk.length; j += 4096) {
            const position = offset + delivered + j;
            if (position >= 44 && chunk[j] !== ((position - 44) % 256) * track.seed % 127 + 64)
              throw Error("incorrect_audio");
          }
          delivered += chunk.length;
          audioDelivery.record(index, now, chunk.length);
          if (now >= steady && now <= end) {
            bytes += chunk.length;
            mediaBytesByOrigin[track.source] = (mediaBytesByOrigin[track.source] || 0) + chunk.length;
          }
          const remaining = began + delivered / readRate * 1000 - performance.now();
          if (remaining > 0) await delay(remaining, undefined, { signal: controller.signal });
        }
        if (offset + delivered !== track.size) throw Error("short_audio");
        offset = 0;
      } catch (error) {
        if (!controller.signal.aborted) {
          if (delivered) interruptions++; else failedStarts++;
          audioDelivery.failed(index, delivered > 0);
          countError("media", error);
          await delay(250);
        }
      } finally {
        res?.destroy();
        if (counted) openResponses--;
      }
    }
  } finally {
    sessions--;
  }
}

function quantiles(values) {
  values.sort((a, b) => a - b);
  const p = (q) => values.length ? Number(values[Math.ceil(values.length * q) - 1].toFixed(2)) : null;
  return { samples: values.length, p50: p(.5), p95: p(.95), p99: p(.99) };
}

const readers = Array.from({ length: clients }, (_, i) => session(i));
const dispatch = setInterval(() => {
  const now = performance.now();
  while (start + issued * 1000 / rate <= now && start + issued * 1000 / rate < end) {
    const index = issued++;
    const scheduled = start + index * 1000 / rate;
    if (scheduled >= steady) {
      due++;
      dispatchLag.push(now - scheduled);
    }
    if (queries >= pendingLimit || now >= end || now - scheduled > 1000) {
      if (scheduled >= steady) omitted++;
      continue;
    }
    if (scheduled >= steady) searchStarts++;
    const query = search(index, scheduled);
    pending.add(query);
    query.finally(() => pending.delete(query));
  }
}, 5);
const sampling = setInterval(() => {
  samples.push({ ms: Math.round(performance.now() - start), media_sessions: sessions,
    open_media_responses: openResponses, search_inflight: queries, interruptions, errors: { ...errors },
    rss_bytes: process.memoryUsage().rss, useful_media_bytes: bytes });
}, 1000);

await delay(ramp);
console.log(JSON.stringify({ phase: "steady", clients, search_per_second: rate, duration_s: duration / 1000,
  end_unix_ms: performance.timeOrigin + end }));
await delay(Math.max(1, end - performance.now()));
clearInterval(dispatch);
clearInterval(sampling);
controller.abort();
agent.destroy();
await Promise.allSettled([...readers, ...pending]);
loop.disable();

const steadySamples = samples.filter(s => s.ms >= ramp + 1000 && s.ms <= ramp + duration);
const sampleTimes = [ramp + 1000, ...steadySamples.map(s => s.ms), ramp + duration];
const maxSampleGap = Math.max(...sampleTimes.slice(1).map((time, i) => time - sampleTimes[i]));
const load = {
  dispatched_search_per_second: searchStarts / (duration / 1000),
  completed_search_per_second: searchTimes.length / (duration / 1000),
  target_readers: clients, observed_open_media: quantiles(steadySamples.map(s => s.open_media_responses)),
  samples_at_target: steadySamples.filter(s => s.open_media_responses === clients).length,
  steady_samples: steadySamples.length, steady_seconds: duration / 1000, max_sample_gap_ms: maxSampleGap,
  samples_with_all_reader_sessions: steadySamples.filter(s => s.media_sessions === clients).length,
  target_bytes_per_second_per_reader: readRate, open_media_peak: peakOpen,
};
const generator = {
  rss_peak_bytes: Math.max(0, ...samples.map(s => s.rss_bytes)), cpu: process.cpuUsage(startedCpu),
  event_loop_utilization: performance.eventLoopUtilization(startedLoop).utilization,
  event_loop_delay_p99_ms: loop.percentile(99) / 1e6, search_dispatch_lag_ms: quantiles(dispatchLag),
};
const expectedSearches = rate * duration / 1000;
console.log(JSON.stringify({
  phase: "complete", profile: cfg.profile, load, generator, search_http_ms: quantiles(searchTimes),
  search_scheduled_to_response_ms: quantiles(scheduledTimes), first_audio_byte_ms: quantiles(firstBytes),
  steady_first_audio_byte_ms: quantiles(steadyFirstBytes), omissions: omitted + Math.max(0, expectedSearches - due),
  search_due: expectedSearches, search_started: searchStarts, search_completed: searchTimes.length,
  search_failed: searchErrors, search_open_at_deadline: Math.max(0, searchStarts - searchErrors - searchTimes.length),
  media_attempts: mediaAttempts, media_starts: mediaStarts, media_failed_starts: failedStarts, interruptions,
  useful_bytes: bytes, per_reader_delivery: audioDelivery.results(), client_audio_bytes_by_publication_origin: mediaBytesByOrigin, errors, samples,
}));
