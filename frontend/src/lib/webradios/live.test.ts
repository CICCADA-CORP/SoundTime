import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { live } from "./live.svelte";
import { webPreferences } from "./preferences.svelte";
import type { WebradioStation } from "./types";

const { api, cookie, hls } = vi.hoisted(() => ({
  api: { post: vi.fn(), get: vi.fn(), put: vi.fn() }, cookie: vi.fn(),
  hls: { supported: true, destroy: vi.fn(), url: vi.fn(), instances: 0 },
}));
vi.mock("$lib/api", () => ({ api, prepareMediaSession: cookie }));
vi.mock("hls.js/light", () => ({
  default: class {
    static Events = { ERROR: "error", MEDIA_ATTACHED: "attach", MANIFEST_PARSED: "manifest" };
    static isSupported = () => hls.supported;
    handlers: Record<string, () => void> = {};
    constructor() { hls.instances++; }
    on(key: string, handler: () => void) { this.handlers[key] = handler; }
    attachMedia() { this.handlers.attach(); this.handlers.manifest(); }
    loadSource = hls.url;
    destroy = hls.destroy;
  }
}));

const id = "77000000-0000-4000-8000-000000000001";
const station: WebradioStation = { id, name: "Waves FM", icon_url: null, description: "", tags: [], country: "",
  language: "", codec: "AAC", bitrate: 128, hls: false };
const enabled = { instance_enabled: true, user_enabled: true, enabled: true };
let audio: TestAudio;
let nativeHls = false;
class TestAudio {
  constructor() { audio = this; }
  src = "";
  volume = 1;
  onplaying?: () => void;
  play = vi.fn(async () => this.onplaying?.());
  pause = vi.fn();
  load = vi.fn();
  removeAttribute = vi.fn(() => { this.src = ""; });
  canPlayType = () => nativeHls ? "maybe" : "";
}

beforeEach(() => {
  window.dispatchEvent(new Event("soundtime:logout"));
  localStorage.clear();
  vi.clearAllMocks();
  hls.instances = 0; hls.supported = true; nativeHls = false;
  vi.stubGlobal("Audio", TestAudio);
  vi.stubGlobal("location", new URL("https://instance.test"));
});
afterEach(() => { live.stop(); vi.unstubAllGlobals(); });

it("loads the light MSE engine for non-native HLS, plays the chosen URL and disposes it", async () => {
  api.post.mockResolvedValue({ url: "https://station.test/live.m3u8", hls: true });
  const restore = vi.fn();
  await live.start(station, 0.31, restore);
  expect(live.phase).toBe("playing");
  expect(hls.url).toHaveBeenCalledWith("https://station.test/live.m3u8");
  expect(audio.volume).toBe(0.31);
  live.setVolume(0.6);
  expect(audio.volume).toBe(0.6);
  live.stop();
  expect(hls.destroy).toHaveBeenCalledOnce();
  expect(audio.pause).toHaveBeenCalledOnce();
  expect(restore).toHaveBeenCalledOnce();
});

it("prefers native HLS and offers the verified external URL if playback is unsupported", async () => {
  api.post.mockResolvedValue({ url: "https://station.test/live.m3u8", hls: true });
  nativeHls = true;
  await live.start(station, 1, vi.fn());
  expect(audio.src).toBe("https://station.test/live.m3u8");
  expect(hls.instances).toBe(0);
  hls.supported = nativeHls = false;
  await live.start(station, 1, vi.fn());
  expect(live.phase).toBe("unsupported");
  expect(live.url).toBe("https://station.test/live.m3u8");
  expect(audio.play).not.toHaveBeenCalled();
});

it("accepts only the matching same-origin relay on HTTPS and renews media access before playback", async () => {
  const relay = `/api/webradios/stations/${id}/stream`;
  for (const bad of ["/api/tracks/other/stream", "//third.party/relay", `${relay}?other=1`]) {
    api.post.mockResolvedValue({ url: "http://station.test/live", relay_url: bad, hls: false });
    await live.start(station, 1, vi.fn());
    expect(live.phase).toBe("unsupported");
    expect(audio.play).not.toHaveBeenCalled();
  }
  api.post.mockResolvedValue({ url: "http://station.test/live", relay_url: relay, hls: false });
  await live.start(station, 1, vi.fn());
  expect(cookie).toHaveBeenCalledOnce();
  expect(audio.src).toBe(relay);
  expect(live.phase).toBe("playing");
});

it("ignores a late resolution after stop and releases a playing guest or member stream on opt-out", async () => {
  let resolve!: (stream: { url: string; hls: boolean }) => void;
  api.post.mockReturnValueOnce(new Promise(done => { resolve = done; }));
  const first = live.start(station, 1, vi.fn());
  live.stop();
  resolve({ url: "https://station.test/stale", hls: false });
  await first;
  expect(audio.play).not.toHaveBeenCalled();
  api.get.mockResolvedValue(enabled);
  api.post.mockResolvedValue({ url: "https://station.test/new", hls: false });
  await webPreferences.refresh(null);
  await live.start(station, 1, vi.fn());
  await webPreferences.setEnabled(false, null);
  expect(live.station).toBeNull();
  expect(localStorage.getItem("soundtime_webradios_guest")).toBe("false");
  expect(api.put).not.toHaveBeenCalled();
  await webPreferences.refresh("member");
  expect(webPreferences.enabled).toBe(true);
  await live.start(station, 1, vi.fn());
  api.put.mockResolvedValue({ ...enabled, user_enabled: false, enabled: false });
  await webPreferences.setEnabled(false, "member");
  expect(api.put).toHaveBeenCalledWith("/webradios/preference", { enabled: false });
  expect(live.station).toBeNull();
});
