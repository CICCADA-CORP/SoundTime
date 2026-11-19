import { stationUrl } from "./types";

export interface PodcastShow {
  id: string;
  title: string;
  author: string;
  artwork_url: string | null;
  genre: string;
}

export interface PodcastEpisode {
  id: string;
  title: string;
  description: string;
  published_at: string | null;
  audio_url: string;
  duration: number | null;
}

type Phase = "loading" | "playing" | "paused" | "failed" | "unsupported";
let show = $state.raw<PodcastShow | null>(null);
let market = $state<string | undefined>();
let episode = $state.raw<PodcastEpisode | null>(null);
let phase = $state<Phase>("loading");
let progress = $state(0);
let duration = $state(0);
let url = $state<string | null>(null);
let audio: HTMLAudioElement | null = null;
let release: (() => void) | undefined;
let timeout: ReturnType<typeof setTimeout> | undefined;

function playbackState(value: MediaSessionPlaybackState) {
  if ("mediaSession" in navigator) navigator.mediaSession.playbackState = value;
}

function updateSystemPosition() {
  if (!audio || !("mediaSession" in navigator) || duration <= 0) return;
  try {
    navigator.mediaSession.setPositionState({ duration, playbackRate: 1, position: Math.min(progress, duration) });
  } catch { /* Older browsers may not implement position controls. */ }
}

function unload() {
  clearTimeout(timeout);
  const element = audio;
  audio = null;
  if (!element) return;
  element.onplaying = element.onpause = element.onended = element.onerror = null;
  element.ontimeupdate = element.onloadedmetadata = element.ondurationchange = null;
  element.pause();
  element.removeAttribute("src");
  element.load();
}

function stop() {
  if (!episode) return;
  unload();
  episode = show = null;
  url = null;
  progress = duration = 0;
  release?.();
  release = undefined;
}

function fail() {
  phase = "failed";
  unload();
  playbackState("paused");
}

async function resume() {
  const element = audio;
  if (!element?.src || phase === "playing") return;
  clearTimeout(timeout);
  const deadline = setTimeout(() => { if (audio === element) fail(); }, 30_000);
  timeout = deadline;
  phase = "loading";
  try {
    await element.play();
  } catch (error) {
    if (audio !== element || timeout !== deadline) return;
    clearTimeout(timeout);
    if (error instanceof DOMException && (error.name === "NotAllowedError" || error.name === "AbortError")) {
      phase = "paused";
      playbackState("paused");
    } else {
      fail();
    }
  }
}

function pause() {
  if (!audio?.src) return;
  clearTimeout(timeout);
  timeout = undefined;
  audio.pause();
  phase = "paused";
  playbackState("paused");
}

function seek(seconds: number) {
  if (!audio || duration <= 0 || !Number.isFinite(seconds)) return;
  const next = Math.max(0, Math.min(seconds, duration));
  try {
    audio.currentTime = next;
    progress = next;
    updateSystemPosition();
  } catch { /* Metadata may still be arriving. */ }
}

async function start(nextShow: PodcastShow, next: PodcastEpisode, volume: number,
  authorize: () => Promise<boolean>, onRelease: () => void, country?: string) {
  stop();
  show = nextShow;
  market = country;
  episode = next;
  duration = next.duration && Number.isFinite(next.duration) && next.duration > 0 ? next.duration : 0;
  phase = "loading";
  release = onRelease;
  const element = new Audio();
  audio = element;
  element.volume = volume;
  element.preload = "none";
  const current = () => audio === element;
  try {
    const permitted = await authorize();
    if (!current()) return;
    if (!permitted) return stop();
    const direct = stationUrl(next.audio_url, true);
    if (!direct) return fail();
    url = direct;
    if (direct.startsWith("http:") && location.protocol === "https:") {
      phase = "unsupported";
      unload();
      return;
    }
    element.onplaying = () => {
      if (!current()) return;
      clearTimeout(timeout);
      phase = "playing";
      playbackState("playing");
      updateSystemPosition();
    };
    element.onpause = () => {
      if (!current()) return;
      phase = "paused";
      playbackState("paused");
    };
    element.ondurationchange = element.onloadedmetadata = () => {
      if (current() && Number.isFinite(element.duration) && element.duration > 0) {
        duration = element.duration;
        updateSystemPosition();
      }
    };
    element.ontimeupdate = () => {
      if (current() && Number.isFinite(element.currentTime)) progress = element.currentTime;
    };
    element.onended = () => { if (current()) stop(); };
    element.onerror = () => { if (current()) fail(); };
    element.src = direct;
    await resume();
  } catch {
    if (current()) fail();
  }
}

export const podcast = {
  get show() { return show; },
  get episode() { return episode; },
  get country() { return market; },
  get showUrl() {
    if (!show) return "/webradios?view=podcasts";
    const params = new URLSearchParams({ view: "podcasts", show: show.id });
    if (market && /^[a-z]{2}$/i.test(market)) params.set("country", market.toUpperCase());
    return "/webradios?" + params.toString();
  },
  get phase() { return phase; },
  get duration() { return duration; },
  get progress() { return progress; },
  get url() { return url; },
  start, pause, resume, stop, seek,
  setVolume(value: number) { if (audio) audio.volume = value; },
};
