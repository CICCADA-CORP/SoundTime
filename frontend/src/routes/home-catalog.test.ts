import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import Home from "./+page.svelte";

const playQueue = vi.hoisted(() => vi.fn());
vi.mock("$lib/stores/queue.svelte", () => ({ getQueueStore: () => ({ playQueue }) }));
vi.mock("$lib/stores/auth.svelte", () => ({ getAuthStore: () => ({ isAuthenticated: true, user: { id: "member" } }) }));
afterEach(() => vi.unstubAllGlobals());

it("links a signed-in listener to the live catalog and plays their recent history", async () => {
  setLocale("en");
  const track = { id: "night", title: "Night Song", artist_id: "anne", artist_name: "Anne", duration_secs: 90, format: "flac", play_count: 2 };
  const previous = { ...track, id: "past", title: "Previously played" };
  const feeds: Record<string, unknown> = {
    "/api/tracks/popular": { data: [track] },
    "/api/tracks/random": [],
    "/api/history/recent": [{ track: previous }],
    "/api/albums/recent": { data: [{ id: "dawn", title: "Dawn" }] },
    "/api/artists/top": { data: [{ id: "anne", name: "Anne" }] },
    "/api/editorial-playlists": [{ id: "curated", name: "Night Set", description: "After dusk", track_count: 1, tracks: [track] }],
    "/api/stats/overview": { total_tracks: 23, total_albums: 5, total_artists: 2, total_duration_secs: 7200 },
    "/api/favorites/ids": []
  };
  vi.stubGlobal("fetch", vi.fn(async (url: string) => Response.json(feeds[new URL(url, "http://localhost").pathname] ?? [])));
  render(Home);

  const recent = await screen.findByRole("button", { name: /Previously played/ });
  await fireEvent.click(recent);
  expect(playQueue).toHaveBeenCalledWith([previous], 0, "collection");
  expect(screen.getByRole("link", { name: /Night Set/ })).toHaveAttribute("href", "/playlists/curated");
  await fireEvent.click(screen.getByRole("tab", { name: "Albums" }));
  expect(screen.getByRole("link", { name: /Dawn/ })).toHaveAttribute("href", "/albums/dawn");
  await fireEvent.click(screen.getByRole("tab", { name: "Artists" }));
  expect(screen.getAllByRole("link", { name: /Anne/ }).some(link => link.getAttribute("href") === "/artists/anne")).toBe(true);
  await fireEvent.click(screen.getByRole("button", { name: "Listen now" }));
  expect(playQueue).toHaveBeenLastCalledWith([track], 0, "playlist");
});
