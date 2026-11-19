import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { getPlayerStore } from "$lib/stores/player.svelte";
import { getQueueStore } from "$lib/stores/queue.svelte";
import { podcast, type PodcastShow, type PodcastEpisode } from "./podcast.svelte";
import { webPreferences } from "./preferences.svelte";
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
const show: PodcastShow = { id:"123", title:"Stories", author:"Author", artwork_url:null, genre:"Culture" };
const episode: PodcastEpisode = { id:"guid-1", title:"Episode 1", audio_url:"https://public.example/episode.mp3", description:"", published_at:null, duration:600 };
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
afterEach(() => { live.stop(); podcast.stop(); vi.unstubAllGlobals(); });

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

it("parks the catalogue, lets the listener seek and pause an episode, then restores the catalogue time", async () => {
  const player = getPlayerStore();
  const queue = getQueueStore();
  vi.mocked(api.get).mockResolvedValue({ enabled: true, instance_enabled: true, user_enabled: true });
  await webPreferences.refresh(null);
  queue.playQueue([music], 0);
  const catalogAudio = instances[0];
  catalogAudio.currentTime = 19;
  player.playPodcast(show, episode, "fr");
  await vi.waitFor(() => expect(podcast.phase).toBe("playing"));
  expect(podcast.showUrl).toBe("/webradios?view=podcasts&show=123&country=FR");
  expect(player.currentTrack).toBeNull();
  expect(podcast.duration).toBe(600);
  const episodeAudio = instances[1] as unknown as HTMLAudioElement;
  episodeAudio.ondurationchange?.call(episodeAudio, new Event("durationchange"));
  expect(podcast.duration).toBe(190);
  podcast.seek(55);
  expect(episodeAudio.currentTime).toBe(55);
  podcast.pause();
  expect(podcast.phase).toBe("paused");
  await podcast.resume();
  expect(podcast.phase).toBe("playing");
  podcast.seek(2000);
  expect(episodeAudio.currentTime).toBe(190);
  episodeAudio.onended?.call(episodeAudio, new Event("ended"));
  expect(podcast.episode).toBeNull();
  expect(queue.queue.map(t => t.id)).toEqual([music.id]);
  expect(player.currentTrack?.id).toBe(music.id);
  expect(player.isPlaying).toBe(false);
  player.resume();
  catalogAudio.dispatchEvent(new Event("loadedmetadata"));
  expect(catalogAudio.currentTime).toBe(19);
  expect(lastfmApi.nowPlaying).toHaveBeenCalledTimes(1);
  expect(api.post).not.toHaveBeenCalledWith("/history", expect.anything());
});

it("releases live radio for an episode, an episode for radio, and active playback on guest opt-out", async () => {
  const player = getPlayerStore();
  vi.mocked(api.get).mockResolvedValue({ enabled: true, instance_enabled: true, user_enabled: true });
  await webPreferences.refresh(null);
  player.playStation(station);
  await vi.waitFor(() => expect(live.phase).toBe("playing"));
  player.playPodcast(show, episode);
  await vi.waitFor(() => expect(podcast.phase).toBe("playing"));
  expect(live.station).toBeNull();
  const firstEpisode = instances.at(-1)!;
  player.playStation(station);
  expect(podcast.episode).toBeNull();
  expect(firstEpisode.src).toBe("");
  player.playPodcast(show, episode);
  await vi.waitFor(() => expect(podcast.phase).toBe("playing"));
  await webPreferences.setEnabled(false, null);
  expect(podcast.episode).toBeNull();
  expect(instances.at(-1)!.src).toBe("");
  await webPreferences.setEnabled(true, null);
});

it("does not open an enclosure on denied access or after a newer track supersedes its permission check", async () => {
  vi.mocked(api.get).mockResolvedValue({ enabled: true, instance_enabled: true, user_enabled: true });
  await webPreferences.refresh(null);
  const player = getPlayerStore();
  let finish!: (value: unknown) => void;
  vi.mocked(api.get).mockReturnValueOnce(new Promise(done => { finish = done; }));
  player.playPodcast(show, episode);
  const stale = instances.at(-1)!;
  player.play(music);
  finish({ enabled: true, instance_enabled: true, user_enabled: true });
  await webPreferences.refresh(null);
  expect(stale.src).toBe("");
  expect(stale.play).not.toHaveBeenCalled();
  expect(player.currentTrack?.id).toBe(music.id);
  vi.mocked(api.get).mockResolvedValue({ enabled: false, instance_enabled: false, user_enabled: true });
  player.playPodcast(show, episode);
  await vi.waitFor(() => expect(podcast.episode).toBeNull());
  expect(instances.at(-1)!.src).toBe("");
});

it("offers an external link for HTTP on HTTPS and discards unsafe enclosures", async () => {
  vi.stubGlobal("location", new URL("https://instance.test"));
  vi.mocked(api.get).mockResolvedValue({ enabled: true, instance_enabled: true, user_enabled: true });
  await webPreferences.refresh(null);
  const player = getPlayerStore();
  player.playPodcast(show, { ...episode, audio_url: "http://public.example/episode.mp3" });
  await vi.waitFor(() => expect(podcast.phase).toBe("unsupported"));
  expect(podcast.url).toBe("http://public.example/episode.mp3");
  expect(instances.at(-1)?.play).not.toHaveBeenCalled();
  player.playPodcast(show, { ...episode, audio_url: "javascript:alert(1)" });
  await vi.waitFor(() => expect(podcast.phase).toBe("failed"));
  expect(podcast.url).toBeNull();
  expect(instances.at(-1)?.play).not.toHaveBeenCalled();
});

it("routes system pause, seek and resume to the episode and restores the track metadata on stop", async () => {
  vi.mocked(api.get).mockResolvedValue({ enabled: true, instance_enabled: true, user_enabled: true });
  await webPreferences.refresh(null);
  const handlers = new Map<MediaSessionAction, MediaSessionActionHandler>();
  const mediaSession = {
    metadata: null as MediaMetadata | null, playbackState: "none", setPositionState: vi.fn(),
    setActionHandler(action: MediaSessionAction, handler: MediaSessionActionHandler | null) {
      if (handler) handlers.set(action, handler);
      else handlers.delete(action);
    },
  };
  vi.stubGlobal("navigator", { mediaSession });
  vi.stubGlobal("MediaMetadata", class { constructor(data: MediaMetadataInit) { Object.assign(this, data); } });
  const player = getPlayerStore();
  player.play(music);
  player.playPodcast(show, episode);
  await vi.waitFor(() => expect(podcast.phase).toBe("playing"));
  expect(mediaSession.metadata?.title).toBe(episode.title);
  handlers.get("pause")!({ action: "pause" });
  expect(podcast.phase).toBe("paused");
  handlers.get("seekto")!({ action: "seekto", seekTime: 80 });
  handlers.get("seekbackward")!({ action: "seekbackward", seekOffset: 30 });
  handlers.get("seekforward")!({ action: "seekforward" });
  expect(podcast.progress).toBe(60);
  expect(mediaSession.setPositionState).toHaveBeenLastCalledWith({ duration:600, position:60, playbackRate:1 });
  handlers.get("play")!({ action:"play" });
  expect(podcast.phase).toBe("playing");
  handlers.get("stop")!({ action:"stop" });
  expect(mediaSession.metadata?.title).toBe(music.title);
  expect(handlers.has("stop")).toBe(false);
  expect(player.currentTrack?.id).toBe(music.id);
});
