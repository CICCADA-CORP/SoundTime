import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import { podcast, type PodcastEpisode, type PodcastShow } from "./podcast.svelte";
import PodcastPlayer from "./PodcastPlayer.svelte";

const show: PodcastShow = { id: "100", title: "On Air", author: "Voice", genre: "Music", artwork_url: null };
const episode: PodcastEpisode = { id: "part-1", title: "Episode one", audio_url: "https://station.example/one.mp3", duration: 3723, description: "", published_at: null };
class AudioSample {
  src = ""; currentTime = 0; onplaying: (() => void) | null = null;
  async play() { this.onplaying?.(); }
  pause() {}
  removeAttribute() { this.src = ""; }
  load() {}
}
afterEach(() => { podcast.stop(); vi.unstubAllGlobals(); });
it("keeps playback, programme navigation and a labelled timeline available away from the directory", async () => {
  setLocale("en");
  vi.stubGlobal("Audio", AudioSample);
  await podcast.start(show, episode, 0.6, async () => true, () => {}, "US");
  render(PodcastPlayer);
  const section = screen.getByRole("region");
  expect(section).toHaveTextContent("Episode one");
  expect(section).toHaveTextContent("1:02:03");
  expect(screen.getByRole("link", { name: show.title })).toHaveAttribute("href", "/webradios?view=podcasts&show=100&country=US");
  const seek = screen.getAllByRole("slider").find(s => s.getAttribute("max") === "3723")!;
  expect(seek).toHaveAccessibleName();
  await fireEvent.input(seek, { target: { value: "75" } });
  expect(podcast.progress).toBe(75);
  const [toggle, stop] = screen.getAllByRole("button");
  expect(toggle).toHaveAccessibleName();
  await fireEvent.click(toggle);
  expect(podcast.phase).toBe("paused");
  await fireEvent.click(toggle);
  expect(podcast.phase).toBe("playing");
  await fireEvent.click(stop);
  expect(podcast.episode).toBeNull();
});

it("keeps an episode paused when the browser asks for a gesture, then plays on request", async () => {
  const play = vi.fn().mockRejectedValueOnce(new DOMException("Gesture required", "NotAllowedError"));
  class GestureAudio extends AudioSample {
    async play() { await play(); this.onplaying?.(); }
  }
  vi.stubGlobal("Audio", GestureAudio);
  const authorize = vi.fn(async () => true);
  await podcast.start(show, episode, 1, authorize, () => {});
  expect(podcast.phase).toBe("paused");
  await podcast.resume();
  expect(podcast.phase).toBe("playing");
  expect(authorize).toHaveBeenCalledOnce();
});

it("releases silent audio and retains the external enclosure after the connection deadline", async () => {
  vi.useFakeTimers();
  class SilentAudio extends AudioSample { async play() {} }
  vi.stubGlobal("Audio", SilentAudio);
  try {
    await podcast.start(show, { ...episode, duration: null }, 1, async () => true, () => {});
    expect(podcast.phase).toBe("loading");
    expect(podcast.duration).toBe(0);
    podcast.seek(25);
    expect(podcast.progress).toBe(0);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(podcast.phase).toBe("failed");
    expect(podcast.url).toBe(episode.audio_url);
  } finally {
    vi.useRealTimers();
  }
});
