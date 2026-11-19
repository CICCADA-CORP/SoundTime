import type Hls from "hls.js";
import { api, prepareMediaSession } from "$lib/api";
import { stationUrl, type WebradioStation } from "./types";

type LivePhase = "loading" | "playing" | "failed" | "unsupported";
let station = $state.raw<WebradioStation | null>(null);
let phase = $state<LivePhase>("loading");
let stream = $state<string | null>(null);
let audio: HTMLAudioElement | null = null;
let hls: Hls | null = null;
let release: (() => void) | undefined;
let generation = 0;
let connectionTimer: ReturnType<typeof setTimeout> | undefined;

function unload() {
  clearTimeout(connectionTimer);
  connectionTimer = undefined;
  if (!audio) return;
  const element = audio;
  audio = null;
  element.onplaying = element.onerror = element.onended = null;
  const engine = hls;
  hls = null;
  engine?.destroy();
  element.pause();
  element.removeAttribute("src");
  element.load();
}

function stop() {
  if (!station) return;
  generation++;
  unload();
  station = null;
  stream = null;
  release?.();
  release = undefined;
}

async function start(next: WebradioStation, volume: number, onRelease: () => void) {
  stop();
  const request = ++generation;
  station = next;
  phase = "loading";
  stream = null;
  release = onRelease;
  const element = new Audio();
  audio = element;
  element.volume = volume;
  element.preload = "none";
  const current = () => request === generation && audio === element;
  connectionTimer = setTimeout(() => {
    if (!current() || phase !== "loading") return;
    phase = "failed";
    unload();
  }, 30_000);
  try {
    const target = await api.post<{ url: string; hls: boolean; relay_url?: string | null }>(`/webradios/stations/${encodeURIComponent(next.id)}/play`);
    if (!current()) return;
    const direct = stationUrl(target.url, true);
    if (!direct) throw new Error("Invalid live URL");
    stream = direct;
    let url = direct;
    if (direct.startsWith("http:") && location.protocol === "https:") {
      const expected = `/api/webradios/stations/${next.id}/stream`;
      if (target.hls || target.relay_url !== expected || !/^[a-f\d]{8}-(?:[a-f\d]{4}-){3}[a-f\d]{12}$/i.test(next.id)) {
        phase = "unsupported";
        unload();
        return;
      }
      await prepareMediaSession();
      if (!current()) return;
      url = expected;
    }
    const failed = () => {
      if (!current()) return;
      phase = "failed";
      unload();
      if ("mediaSession" in navigator) navigator.mediaSession.playbackState = "paused";
    };
    element.onplaying = () => {
      if (!current()) return;
      clearTimeout(connectionTimer);
      phase = "playing";
      if ("mediaSession" in navigator) navigator.mediaSession.playbackState = "playing";
    };
    element.onerror = element.onended = failed;
    if (target.hls && !element.canPlayType("application/vnd.apple.mpegurl") && !element.canPlayType("application/x-mpegURL")) {
      const { default: HlsLight } = await import("hls.js/light");
      if (!current()) return;
      if (!HlsLight.isSupported()) {
        phase = "unsupported";
        unload();
        return;
      }
      const engine = new HlsLight({ enableWorker: false, maxBufferLength: 20, maxMaxBufferLength: 30, backBufferLength: 10 });
      hls = engine;
      engine.on(HlsLight.Events.ERROR, (_, error) => { if (error.fatal) failed(); });
      engine.on(HlsLight.Events.MEDIA_ATTACHED, () => { if (current()) engine.loadSource(url); });
      engine.on(HlsLight.Events.MANIFEST_PARSED, () => { if (current()) void element.play().catch(failed); });
      engine.attachMedia(element);
    } else {
      element.src = url;
      await element.play();
    }
  } catch (error) {
    if (!current()) return;
    if ([401, 403].includes((error as { status?: number })?.status ?? 0)) {
      stop();
      window.dispatchEvent(new Event("soundtime:webradio-denied"));
      return;
    }
    phase = "failed";
    unload();
  }
}

export const live = {
  get station() { return station; },
  get phase() { return phase; },
  get url() { return stream; },
  start,
  stop,
  setVolume(value: number) { if (audio) audio.volume = value; },
};
