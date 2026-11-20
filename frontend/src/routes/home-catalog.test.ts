import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import Home from "./+page.svelte";

const playQueue = vi.hoisted(() => vi.fn());
const addToQueue = vi.hoisted(() => vi.fn());
vi.mock("$lib/stores/queue.svelte", () => ({ getQueueStore: () => ({ playQueue, addToQueue }) }));
vi.mock("$lib/stores/auth.svelte", () => ({ getAuthStore: () => ({ isAuthenticated: true, user: { id: "member" } }) }));
afterEach(() => vi.unstubAllGlobals());

it("links a signed-in listener to the live catalog and plays their recent history", async () => {
  setLocale("en");
  const track = { id: "night", title: "Night Song", artist_id: "anne", artist_name: "Anne", duration_secs: 90, format: "flac", play_count: 2 };
  const previous = { ...track, id: "past", title: "Previously played" };
  const feeds: Record<string, unknown> = {
    "/api/tracks/popular": { data: [track] },
    "/api/tracks/random": [],
    "/api/tracks/recently-added": { data: [] },
    "/api/genres": ["Nu Jazz"],
    "/api/history/recent": [{ track: previous }],
    "/api/albums/recent": { data: [{ id: "dawn", title: "Dawn", artist_id: "anne", cover_url: "/api/media/dawn.webp" }] },
    "/api/artists/top": { data: [{ id: "anne", name: "Anne" }] },
    "/api/editorial-playlists": [{ id: "curated", name: "Night Set", description: "After dusk", track_count: 1, tracks: [track] }],
    "/api/stats/overview": { total_tracks: 23, total_albums: 5, total_artists: 2, total_duration_secs: 7200 },
    "/api/favorites/ids": []
  };
  vi.stubGlobal("fetch", vi.fn(async (url: string) => Response.json(feeds[new URL(url, "http://localhost").pathname] ?? [])));
  render(Home);

  const recent = await screen.findByRole("button", { name: /Previously played/ });
  expect(screen.getByRole("heading", { name: "Recently Played" })).toBeVisible();
  expect(screen.getByRole("img", {name: "Cover of Dawn"})).toHaveAttribute("src", "/api/media/dawn.webp");
  await fireEvent.click(screen.getByRole("button", { name: "Catalog" }));
  expect(screen.getByRole("link", { name: "Nu Jazz" })).toHaveAttribute("href", "/explore?tab=genres&genre=Nu+Jazz");
  expect(screen.getByRole("link", { name: /View all.*Most played/i })).toHaveAttribute("href", "/explore?tab=trending&period=all");
  await fireEvent.click(screen.getByRole("button", { name: "Add to queue · Night Song" }));
  expect(addToQueue).toHaveBeenCalledWith(track);
  await fireEvent.click(recent);
  expect(playQueue).toHaveBeenCalledWith([previous], 0, "collection");
  expect(screen.getByRole("link", { name: /Night Set/ })).toHaveAttribute("href", "/playlists/curated");
  await fireEvent.click(screen.getByRole("tab", { name: "Albums" }));
  expect(within(screen.getByRole("tabpanel", { name: "Albums" })).getByRole("link", { name: /Dawn/ })).toHaveAttribute("href", "/albums/dawn");
  await fireEvent.click(screen.getByRole("tab", { name: "Artists" }));
  expect(screen.getAllByRole("link", { name: /Anne/ }).some(link => link.getAttribute("href") === "/artists/anne")).toBe(true);
  await fireEvent.click(screen.getByRole("button", { name: "Listen now" }));
  expect(playQueue).toHaveBeenLastCalledWith([track], 0, "playlist");
});

it("uses the available album art from the playable selection and opens its tracklist", async () => {
  setLocale("en");
  const tracks = [
    { id: "archived", title: "Archive cut", artist_name: "A", availability: "withdrawn" },
    { id: "single", title: "Live cut", artist_name: "B", duration_secs: 180, availability: "playable", album_id: "release" },
  ];
  const feeds: Record<string, unknown> = {
    "/api/editorial-playlists": [{ id: "mix", name: "Friday Selection", cover_url: "/api/media/session.webp", tracks }],
    "/api/albums/recent": { data: [
      { id: "other", title: "Another release", cover_url: "/api/media/other.webp" },
      { id: "release", title: "Live record", cover_url: "/api/media/live.webp" },
    ] },
    "/api/stats/overview": { total_tracks: 2 },
    "/api/favorites/ids": [], "/api/history/recent": [], "/api/genres": [], "/api/tracks/random": [],
  };
  vi.stubGlobal("fetch", vi.fn(async (url: string) => Response.json(feeds[new URL(url, "http://localhost").pathname] ?? {data: []})));
  render(Home);
  const feature = await screen.findByRole("region", { name: "Friday Selection" });
  const selection = within(feature);
  expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Listen Now");
  expect(selection.getByRole("heading", { level: 2 })).toHaveTextContent("Friday Selection");
  const card = selection.getByRole("button", { name: "Listen now · Live cut" });
  expect(card.querySelector("img")?.getAttribute("src")).toBe("/api/media/live.webp");
  await fireEvent.click(card);
  expect(playQueue).toHaveBeenLastCalledWith([{ ...tracks[1], cover_url: "/api/media/live.webp" }], 0, "playlist");
  const toggle = selection.getByRole("button", { name: "Tracklist" });
  expect(toggle).not.toHaveAttribute("aria-controls");
  await fireEvent.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  expect(within(selection.getByRole("list")).getByRole("button", { name: /Live cut/ })).toBeVisible();
  expect(selection.queryByText("Archive cut")).toBeNull();
  await fireEvent.click(selection.getByRole("button", { name: "Listen now" }));
  expect(playQueue).toHaveBeenLastCalledWith([{ ...tracks[1], cover_url: "/api/media/live.webp" }], 0, "playlist");
});


it("brings three distinct playable releases with covers forward and labels recent additions accurately", async () => {
  setLocale("en");
  const track = (id: string, cover_url?: string, availability = "playable") => ({
    id, title: id, duration_secs: 182, artist_id: "artist-" + id, artist_name: "Artist " + id, cover_url, availability,
  });
  const popular = [track("plain"), track("first", "/cover/one"), track("second-cut", "/cover/one")];
  const recent = [track("removed", "/cover/hidden", "withdrawn"), track("second", "/cover/two"), track("third", "/cover/three")];
  const feeds: Record<string, unknown> = {
    "/api/tracks/popular": { data: popular }, "/api/tracks/recently-added": { data: recent },
    "/api/tracks/random": [], "/api/history/recent": [], "/api/albums/recent": {data: []},
    "/api/artists/top": {data: []}, "/api/editorial-playlists": [], "/api/genres": [], "/api/stats/overview": {total_tracks: 6},
  };
  vi.stubGlobal("fetch", vi.fn(async (url: string) => Response.json(feeds[new URL(url, "http://localhost").pathname] ?? [])));
  render(Home);
  const first = await screen.findByRole("button", { name: "Listen now · first" });
  const shelf = first.closest("section")!;
  const cards = Array.from(shelf.querySelectorAll<HTMLButtonElement>(".track-card"));
  expect(cards.map(card => card.getAttribute("aria-label"))).toEqual(["Listen now · first", "Listen now · second", "Listen now · third"]);
  expect(cards[0]).toHaveTextContent("Artist first");
  expect(cards[0]).toHaveTextContent("3:02");
  expect(screen.queryByRole("heading", { name: "Recently Played" })).toBeNull();
  expect(screen.getByRole("heading", { name: "Recent tracks" })).toBeVisible();
  await fireEvent.click(cards[2]);
  expect(playQueue).toHaveBeenLastCalledWith([popular[1], recent[1], recent[2], popular[0], popular[2]], 2, "collection");
});
