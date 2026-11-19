import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { getPlayerStore } from "$lib/stores/player.svelte";
import { getQueueStore } from "$lib/stores/queue.svelte";
import { live } from "./live.svelte";
import { api, lastfmApi } from "$lib/api";
import type { Track } from "$lib/types";
import type { WebradioStation } from "./types";

vi.mock("$lib/api", () => ({
  API_BASE: "/api", prepareMediaSession: vi.fn(), streamUrl: (id: string) => "/api/tracks/" + id + "/stream",
  api: { get: vi.fn(), post: vi.fn(async () => ({ url: "https://station.test/audio", hls: false })) },
  lastfmApi: { nowPlaying: vi.fn(async () => {}), scrobble: vi.fn(async () => {}) },
}));
class PlayerAudio extends EventTarget {
  src = ""; volume = 1; currentTime = 0; duration = 190; onplaying?: () => void;
  constructor() { super(); instances.push(this); }
  play = vi.fn(async () => { this.dispatchEvent(new Event("play")); this.onplaying?.(); });
  pause = vi.fn(() => this.dispatchEvent(new Event("pause")));
  removeAttribute = vi.fn(() => { this.src = ""; });
  load = vi.fn();
}
let instances: PlayerAudio[] = [];
const music = { id: "track-one", title: "Song", duration_secs: 190, availability: "playable" } as Track;
const station: WebradioStation = { id:"77000000-0000-4000-8000-000000000001", name:"Waves",
  description:"", tags:[], country:"", language:"", codec:"", bitrate:0, icon_url:null, hls:false };

beforeEach(() => {
  vi.stubGlobal("Audio", PlayerAudio);
  getPlayerStore()._resetForTests();
  getQueueStore().clearQueue();
  vi.clearAllMocks();
  instances = [];
});
afterEach(() => { live.stop(); vi.unstubAllGlobals(); });

it("parks a playing queue at its saved position, excludes stations from track statistics, and resumes on request", async () => {
  const player = getPlayerStore(), queue = getQueueStore();
  queue.playQueue([music, {...music, id:"track-two"}], 0);
  const catalogAudio = instances[0];
  catalogAudio.currentTime = 12;
  catalogAudio.dispatchEvent(new Event("timeupdate"));
  expect(player.progress).toBe(12);
  player.playStation(station);
  await vi.waitFor(() => expect(live.phase).toBe("playing"));
  expect(catalogAudio.pause).toHaveBeenCalled();
  expect(catalogAudio.src).toBe("");
  expect(player.currentTrack).toBeNull();
  expect(queue.queue.map(row=>row.id)).toEqual(["track-one","track-two"]);
  expect(lastfmApi.nowPlaying).toHaveBeenCalledTimes(1);
  expect(api.post).not.toHaveBeenCalledWith("/history",expect.anything());
  const stationAudio = instances[1];
  live.stop();
  expect(stationAudio.src).toBe("");
  expect(player.currentTrack?.id).toBe("track-one");
  expect(player.isPlaying).toBe(false);
  player.resume();
  catalogAudio.dispatchEvent(new Event("loadedmetadata"));
  expect(catalogAudio.currentTime).toBe(12);
  player.playStation(station);
  queue.next();
  expect(live.station).toBeNull();
  expect(player.currentTrack?.id).toBe("track-two");
  window.dispatchEvent(new Event("soundtime:logout"));
  expect(player.currentTrack).toBeNull();
  expect(queue.queue).toHaveLength(0);
});
