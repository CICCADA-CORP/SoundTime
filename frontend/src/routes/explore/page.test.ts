import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import Explore from "./+page.svelte";

const startRadio = vi.hoisted(() => vi.fn());
vi.mock("$lib/stores/radio.svelte", () => ({ getRadioStore: () => ({ startRadio }) }));
afterEach(() => vi.unstubAllGlobals());

it("browses a genre, an editorial collection and different trending periods", async () => {
  setLocale("en");
  const track = (id: string) => ({ id, title: id, artist_id: "artist", duration_secs: 90, format: "mp3", play_count: 2 });
  const fetch = vi.fn(async (url: string) => {
    const { pathname, searchParams } = new URL(url, "http://localhost");
    switch (pathname) {
      case "/api/genres": return Response.json(["Jazz"]);
      case "/api/genres/Jazz/tracks": return Response.json({ data: [track("Jazz selection")] });
      case "/api/tracks/popular": return Response.json({ data: [track("Popular selection")] });
      case "/api/tracks/trending": return Response.json(searchParams.get("window") === "7d" ? [track("Weekly selection")] : []);
      case "/api/editorial-playlists": return Response.json([{
        id: "edit", name: "Evening listening", description: "Curated jazz", track_count: 1, tracks: [track("Editorial selection")]
      }]);
      case "/api/stats/overview": return Response.json(null);
      default: return Response.json({ data: [] });
    }
  });
  vi.stubGlobal("fetch", fetch);
  render(Explore);
  const browse = within(screen.getByRole("navigation", { name: "Explore" }));
  expect(browse.getByRole("link", { name: "Playlists" })).toHaveAttribute("href", "/playlists");
  expect(browse.getByRole("link", { name: "Albums" })).toHaveAttribute("href", "/albums");
  expect(await screen.findByRole("link", { name: "Popular selection" })).toBeVisible();

  await fireEvent.click(screen.getByRole("button", { name: "7 Days" }));
  expect(await screen.findByRole("link", { name: "Weekly selection" })).toBeVisible();
  expect(screen.queryByRole("link", { name: "Popular selection" })).toBeNull();

  const genre = screen.getByRole("button", { name: "Jazz" });
  await fireEvent.click(genre);
  expect(await screen.findByRole("link", { name: "Jazz selection" })).toBeVisible();
  await fireEvent.click(screen.getByRole("button", { name: "Start Radio" }));
  expect(startRadio).toHaveBeenCalledWith("genre", { genre: "Jazz", label: "Jazz" });
  await fireEvent.click(genre);
  expect(screen.queryByRole("link", { name: "Jazz selection" })).toBeNull();

  await fireEvent.click(screen.getByRole("button", { name: /Evening listening/ }));
  expect(await screen.findByRole("link", { name: "Editorial selection" })).toBeVisible();
  await fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(screen.queryByRole("link", { name: "Editorial selection" })).toBeNull();
});

it("keeps a successful shelf visible and retries only failed catalog sections", async () => {
  setLocale("en");
  let requests = 0;
  const fetch = vi.fn(async (url: string) => {
    const path = new URL(url, "http://localhost").pathname;
    if (path === "/api/genres") {
      requests++;
      return requests === 1 ? new Response(null, { status: 503 }) : Response.json(["Recovered genre"]);
    }
    if (path === "/api/editorial-playlists" || path === "/api/tracks/trending") return Response.json([]);
    if (path === "/api/stats/overview") return Response.json(null);
    if (path === "/api/albums/recent") return Response.json({ data: [{ id: "record", title: "Still available" }] });
    return Response.json({ data: [] });
  });
  vi.stubGlobal("fetch", fetch);
  render(Explore);
  const notice = await screen.findByText("Some sections could not be loaded.");
  expect(await screen.findByRole("link", { name: "Still available" })).toBeVisible();
  await fireEvent.click(within(notice).getByRole("button"));
  expect(await screen.findByRole("button", { name: "Recovered genre" })).toBeVisible();
  expect(fetch.mock.calls.filter(([url]) => url.includes("/albums/recent"))).toHaveLength(1);
});
