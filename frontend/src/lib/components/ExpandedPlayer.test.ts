import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { fromStore, writable } from "svelte/store";
import { tick } from "svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import { showcaseTracks } from "$lib/showcase";
import type { Track } from "$lib/types";
import ExpandedPlayer from "./ExpandedPlayer.svelte";

const track = { ...showcaseTracks[0], id: "track-1", artist_id: "artist-1", album_id: "album-1",
  title: "Expanded Song", artist_name: "Expanded Artist", album_title: "Expanded Album",
  format: "flac", bitrate: 1411, sample_rate: 44100, cover_url: "/covers/song.jpg" };
const next = { ...track, id: "track-2", title: "Next song", cover_url: undefined };
const following = { ...track, id: "track-3", title: "Third song" };

const selected = writable<Track | null>(track);
const activeTrack = fromStore(selected);
const player = {
  get currentTrack() { return activeTrack.current; },
  isPlaying: true, progress: 65, duration: 200, shuffle: false, repeat: "none",
  volume: .8, setVolume: vi.fn(), togglePlay: vi.fn(), seek: vi.fn(), toggleShuffle: vi.fn(), cycleRepeat: vi.fn(),
};
const queue = {
  queue: [] as Track[], currentIndex: 0, autoplay: false, sourceContext: "album",
  playQueue: vi.fn(), moveInQueue: vi.fn(), removeFromQueue: vi.fn(), next: vi.fn(), previous: vi.fn(), toggleAutoplay: vi.fn(),
};
const auth = { isAuthenticated: false, user: { id: "me" } };
const radio = { active: false, autoplayMode: false, seedLabel: "Jazz", stopRadio: vi.fn() };
const get = vi.fn();
const post = vi.fn();

vi.mock("$lib/stores/player.svelte", () => ({ getPlayerStore: () => player }));
vi.mock("$lib/stores/queue.svelte", () => ({ getQueueStore: () => queue }));
vi.mock("$lib/stores/auth.svelte", () => ({ getAuthStore: () => auth }));
vi.mock("$lib/stores/radio.svelte", () => ({ getRadioStore: () => radio }));
vi.mock("$lib/api", () => ({ api: { get: (...args: unknown[]) => get(...args), post: (...args: unknown[]) => post(...args) }, API_BASE: "https://media.test/api" }));

function show(open = true, onclose = vi.fn()) {
  render(ExpandedPlayer, { props: { open, onclose } });
  return onclose;
}

function openLyrics() {
  // Mobile and desktop render the same switch in their respective layouts.
  return fireEvent.click(screen.getAllByRole("button", { name: "Lyrics" })[0]);
}

describe("expanded player", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setLocale("en");
    selected.set(track);
    queue.queue = [track];
    player.isPlaying = true;
    player.duration = 200;
    player.progress = 65;
    player.repeat = "none";
    auth.isAuthenticated = false;
    radio.active = false;
    radio.autoplayMode = false;
    get.mockImplementation(async (path: string) =>
      path.startsWith("/favorites") ? [] : path.startsWith("/me/playlists?") ? { data: [] } : { lyrics: null, source: null });
  });

  it("stays closed without a selection or without an explicit open action", () => {
    show(false);
    selected.set(null);
    show();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(get).not.toHaveBeenCalled();
  });

  it("displays artwork, artist, album, progress and format", async () => {
    show();
    const dialog = await screen.findByRole("dialog", { name: "Now Playing" });
    expect(within(dialog).getByRole("heading", { name: track.title })).toBeVisible();
    expect(screen.getByRole("link", { name: track.artist_name })).toHaveAttribute("href", "/artists/artist-1");
    expect(screen.getByRole("link", { name: track.album_title })).toHaveAttribute("href", "/albums/album-1");
    expect(dialog.querySelector("img")).toHaveAttribute("src", "https://media.test/covers/song.jpg");
    for (const text of ["1:05", "3:20", "flac", "1411 kbps", "44.1 kHz"]) expect(screen.getByText(text)).toBeInTheDocument();
    expect(screen.getByText("Queue is empty")).toBeInTheDocument();
  });

  it("keeps cover art and album navigation when an artist ID has no display credit", () => {
    selected.set({ ...track, cover_url: "https://cdn.test/image.webp", artist_name: null, bitrate: null, sample_rate: null });
    player.duration = 0;
    player.progress = 0;
    show();
    expect(screen.getByRole("dialog").querySelector("img")).toHaveAttribute("src", "https://cdn.test/image.webp");
    expect(screen.getByRole("dialog").querySelector('a[href^="/artists/"]')).toBeNull();
    expect(screen.getByRole("link", { name: track.album_title })).toHaveAttribute("href", "/albums/album-1");
    expect(screen.queryByText("Unknown Artist")).toBeNull();
    expect(screen.queryByText(/kbps|kHz/)).toBeNull();
    expect(screen.getByRole("slider", { name: "Playback position" })).toBeDisabled();
  });

  it("uses named, keyboard-accessible transport and seek controls", async () => {
    player.repeat = "one";
    show();
    for (const [label, action] of [
      ["Pause", player.togglePlay], ["Shuffle", player.toggleShuffle], ["Previous", queue.previous],
      ["Next", queue.next], ["Repeat · one", player.cycleRepeat], ["Autoplay", queue.toggleAutoplay],
    ] as const) {
      await fireEvent.click(screen.getByRole("button", { name: label }));
      expect(action).toHaveBeenCalledOnce();
    }
    expect(screen.getByRole("button", { name: "Repeat · one" })).toHaveAttribute("aria-pressed", "true");
    await fireEvent.input(screen.getByRole("slider", { name: "Playback position" }), { target: { value: "50" } });
    expect(player.seek).toHaveBeenCalledWith(50);
    await fireEvent.input(screen.getByRole("slider", { name: "Volume" }), { target: { value: "0.35" } });
    expect(player.setVolume).toHaveBeenCalledWith(.35);
  });

  it("closes on Escape and returns focus to the control that opened it", async () => {
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();
    try {
      const onclose = show();
      const close = await screen.findByRole("button", { name: "Close" });
      await waitFor(() => expect(screen.getByRole("dialog")).toContainElement(document.activeElement as HTMLElement));
      await fireEvent.keyDown(close, { key: "Escape" });
      await waitFor(() => expect(onclose).toHaveBeenCalledOnce());
      await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
      await waitFor(() => expect(opener).toHaveFocus());
    } finally {
      opener.remove();
    }
  });

  it("selects, reorders and removes upcoming tracks while preserving their playback source", async () => {
    queue.queue = [track, next, following, next];
    show();
    const rows = within(screen.getByRole("dialog")).getAllByRole("listitem");
    expect(rows).toHaveLength(3);
    const repeatedSong = screen.getAllByRole("button", { name: /Next song.*Expanded Artist/ });
    await fireEvent.click(repeatedSong[0]);
    expect(queue.playQueue).toHaveBeenCalledWith(queue.queue, 1, "album");
    await fireEvent.click(repeatedSong[1]);
    expect(queue.playQueue).toHaveBeenCalledWith(queue.queue, 3, "album");
    await fireEvent.keyDown(screen.getAllByRole("button", { name: "Options · Next song" })[0], { key: "Enter" });
    expect(await screen.findByRole("menuitem", { name: "Move up" })).toHaveAttribute("aria-disabled", "true");
    await fireEvent.keyDown(screen.getByRole("menuitem", { name: "Move down" }), { key: "Enter" });
    expect(queue.moveInQueue).toHaveBeenCalledWith(1, 2);
    const followingRow = within(rows[1]);
    await fireEvent.keyDown(followingRow.getByRole("button", { name: "Options · Third song" }), { key: "Enter" });
    await fireEvent.click(await followingRow.findByRole("menuitem", { name: "Remove" }));
    expect(queue.removeFromQueue).toHaveBeenCalledWith(2);
  });

  it("shows active radio or autoplay and exposes the stop action", async () => {
    radio.active = true;
    const first = show();
    expect(screen.getByText("Now playing from Jazz")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Stop Radio" }));
    expect(radio.stopRadio).toHaveBeenCalledOnce();
    await fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(first).toHaveBeenCalledOnce();
    radio.autoplayMode = true;
    show();
    expect(screen.getByText("Autoplay")).toBeInTheDocument();
  });

  it("loads lyrics on demand and ignores late responses for a previous track", async () => {
    let finish!: (value: { lyrics: string; source: string }) => void;
    get.mockImplementation((path: string) => path.includes(track.id)
      ? new Promise(resolve => { finish = resolve; })
      : Promise.resolve({ lyrics: "Current lyrics", source: "embedded" }));
    show();
    expect(get).not.toHaveBeenCalled();
    await openLyrics();
    await waitFor(() => expect(get).toHaveBeenCalledWith("/tracks/track-1/lyrics"));
    selected.set(next);
    await screen.findByText("Current lyrics");
    finish({ lyrics: "Obsolete lyrics", source: "old" });
    await tick();
    expect(screen.queryByText("Obsolete lyrics")).toBeNull();
    expect(screen.getByText("Current lyrics")).toBeInTheDocument();
  });

  it("adds the selected real track to an owned playlist", async () => {
    auth.isAuthenticated = true;
    get.mockImplementation(async (path: string) => path.startsWith("/me/playlists?")
      ? { data: [{ id: "mix", name: "My list", user_id: "me" }] } : [track.id]);
    show();
    await screen.findByTitle("Remove from favorites");
    await fireEvent.click(screen.getByRole("button", { name: "Add to playlist" }));
    await fireEvent.click(await screen.findByRole("button", { name: "My list" }));
    expect(post).toHaveBeenCalledWith("/playlists/mix/tracks", { track_id: track.id });
  });

  it("uses local art for previews and keeps lyrics, favorites and playlists off the network", async () => {
    auth.isAuthenticated = true;
    selected.set(showcaseTracks[0]);
    show();
    expect(screen.getByRole("dialog").querySelector("img")).toHaveAttribute("src", showcaseTracks[0].cover_url);
    expect(screen.queryByRole("button", { name: "Add to playlist" })).toBeNull();
    await openLyrics();
    await screen.findByText("No lyrics available");
    expect(get).not.toHaveBeenCalled();
    expect(post).not.toHaveBeenCalled();
  });
});
