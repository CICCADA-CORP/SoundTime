import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { goto } from "$app/navigation";
import { setLocale } from "$lib/i18n/index.svelte";
import Explore from "./+page.svelte";

const startRadio = vi.hoisted(() => vi.fn());
vi.mock("$lib/stores/radio.svelte", () => ({ getRadioStore: () => ({ startRadio }) }));
afterEach(() => vi.unstubAllGlobals());

const track = (id: string) => ({ id, title: id, artist_id: "artist", duration_secs: 90, format: "mp3", play_count: 2 });

it("opens a genre or all-time chart directly from the home shelves", async () => {
  setLocale("en");
  await goto("/explore?tab=genres&genre=Nu+Jazz");
  const fetch = vi.fn(async (url: string) => {
    const path = new URL(url, "http://localhost").pathname;
    if (path === "/api/genres") return Response.json(["Nu Jazz"]);
    return Response.json({ data: [track(path === "/api/tracks/popular" ? "Chart title" : "Jazz title")] });
  });
  vi.stubGlobal("fetch", fetch);
  render(Explore);
  expect(await screen.findByRole("link", { name: "Jazz title" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Nu Jazz" })).toHaveAttribute("aria-pressed", "true");
  await goto("/explore?tab=trending&period=all");
  expect(await screen.findByRole("link", { name: "Chart title" })).toBeVisible();
  expect(screen.getByRole("button", { name: "All time" })).toHaveAttribute("aria-pressed", "true");
  expect(fetch.mock.calls.some(([url]) => url.includes("/tracks/trending"))).toBe(false);
});

it("keeps the loading layout until both recent feeds have settled", async () => {
  setLocale("en");
  await goto("/explore");
  let finishAlbums!: (response: Response) => void;
  const albums = new Promise<Response>(resolve => { finishAlbums = resolve; });
  vi.stubGlobal("fetch", vi.fn(async (url: string) => url.includes("/albums/recent")
    ? albums
    : Response.json({ data: [track("Fast single")] })));
  render(Explore);
  const panel = within(screen.getByRole("tabpanel"));
  expect(await panel.findByRole("status")).toHaveTextContent("Loading");
  expect(panel.queryByRole("link", { name: "Fast single" })).toBeNull();
  finishAlbums(Response.json({ data: [{ id: "record", title: "Slow album" }] }));
  expect(await panel.findByRole("link", { name: "Slow album" })).toBeVisible();
  expect(panel.getByRole("link", { name: "Fast single" })).toBeVisible();
  expect(panel.queryByRole("status")).toBeNull();
});

it("browses a genre, an editorial collection and different trending periods", async () => {
  setLocale("en");
  await goto("/explore");
  let finishDaily!: (response: Response) => void;
  const dailyResponse = new Promise<Response>(resolve => { finishDaily = resolve; });
  const fetch = vi.fn(async (url: string) => {
    const { pathname, searchParams } = new URL(url, "http://localhost");
    switch (pathname) {
      case "/api/genres": return Response.json(["Jazz"]);
      case "/api/albums/recent": return Response.json({ data: [{ id: "album", title: "New album" }] });
      case "/api/tracks/recently-added": return Response.json({ data: [{ ...track("New single"), artist_name: "Studio Ensemble", bitrate: 320 }] });
      case "/api/artists/top": return Response.json({ data: [{ id: "artist", name: "Featured artist" }] });
      case "/api/genres/Jazz/tracks": return Response.json({ data: [track("Jazz selection")] });
      case "/api/tracks/popular": return Response.json({ data: [track("Popular selection")] });
      case "/api/tracks/trending": return searchParams.get("window") === "7d" ? Response.json([track("Weekly selection")]) : dailyResponse;
      case "/api/editorial-playlists": return Response.json([{
        id: "edit", name: "Evening listening", description: "Curated jazz", track_count: 1, tracks: [track("Editorial selection")]
      }]);
      default: return Response.json({ data: [] });
    }
  });
  vi.stubGlobal("fetch", fetch);
  render(Explore);
  const form = screen.getByRole("search") as HTMLFormElement;
  expect(form).toHaveAttribute("action", "/search");
  expect(form.method).toBe("get");
  expect(within(form).getByRole("searchbox")).toHaveAttribute("name", "q");
  const tabs = within(screen.getByRole("tablist", { name: "Explore" }));
  expect(tabs.getByRole("tab", { name: "Recently added" })).toHaveAttribute("aria-selected", "true");
  expect(tabs.getAllByRole("tab")).toHaveLength(5);
  expect(await screen.findByRole("link", { name: "New single" })).toBeVisible();
  const single = within(screen.getByRole("group", { name: "New single" }));
  expect(single.getAllByText("Studio Ensemble")).toHaveLength(1);
  expect(screen.queryByText("Plays")).toBeNull();
  expect(screen.queryByText("Quality")).toBeNull();
  expect(screen.getByRole("link", { name: /New album/ })).toHaveAttribute("href", "/albums/album");
  expect(screen.getAllByRole("link").some(link => link.getAttribute("href") === "/albums")).toBe(true);
  const paths = () => fetch.mock.calls.map(([url]) => new URL(url, "http://localhost").pathname);
  expect(paths().sort()).toEqual(["/api/albums/recent", "/api/tracks/recently-added"]);

  await fireEvent.click(tabs.getByRole("tab", { name: "Trending" }));
  await waitFor(() => expect(paths()).toContain("/api/tracks/trending"));
  expect(paths()).not.toContain("/api/tracks/popular");
  finishDaily(Response.json([]));
  expect(await screen.findByRole("link", { name: "Popular selection" })).toBeVisible();

  await fireEvent.click(screen.getByRole("button", { name: "7 Days" }));
  expect(await screen.findByRole("link", { name: "Weekly selection" })).toBeVisible();
  expect(screen.queryByRole("link", { name: "Popular selection" })).toBeNull();
  expect(paths().filter(path => path === "/api/tracks/popular")).toHaveLength(1);

  await fireEvent.click(tabs.getByRole("tab", { name: "Artists" }));
  expect(await screen.findByRole("link", { name: /Featured artist/ })).toHaveAttribute("href", "/artists/artist");
  expect(screen.getAllByRole("link").some(link => link.getAttribute("href") === "/artists")).toBe(true);
  expect(paths()).not.toContain("/api/genres");
  await fireEvent.click(tabs.getByRole("tab", { name: "Genres" }));
  const genre = await screen.findByRole("button", { name: "Jazz" });
  await fireEvent.click(genre);
  expect(await screen.findByRole("link", { name: "Jazz selection" })).toBeVisible();
  await fireEvent.click(screen.getByRole("button", { name: "Start Radio" }));
  expect(startRadio).toHaveBeenCalledWith("genre", { genre: "Jazz", label: "Jazz" });
  await fireEvent.click(genre);
  expect(screen.queryByRole("link", { name: "Jazz selection" })).toBeNull();

  expect(paths()).not.toContain("/api/editorial-playlists");
  await fireEvent.click(tabs.getByRole("tab", { name: "Playlists" }));
  await fireEvent.click(await screen.findByRole("button", { name: /Evening listening/ }));
  expect(screen.getAllByRole("link").some(link => link.getAttribute("href") === "/playlists")).toBe(true);
  expect(await screen.findByRole("link", { name: "Editorial selection" })).toBeVisible();
  await fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(screen.queryByRole("link", { name: "Editorial selection" })).toBeNull();
  expect(paths()).not.toContain("/api/stats/overview");
});

it("keeps a successful shelf visible and retries only failed catalog sections", async () => {
  setLocale("en");
  await goto("/explore");
  let requests = 0;
  const fetch = vi.fn(async (url: string) => {
    const path = new URL(url, "http://localhost").pathname;
    if (path === "/api/tracks/recently-added") {
      requests++;
      return requests === 1 ? new Response(null, { status: 503 }) : Response.json({ data: [track("Recovered single")] });
    }
    if (path === "/api/albums/recent") return Response.json({ data: [{ id: "record", title: "Still available" }] });
    return Response.json({ data: [] });
  });
  vi.stubGlobal("fetch", fetch);
  render(Explore);
  const notice = await screen.findByText("Some sections could not be loaded.");
  const retry = within(notice).getByRole("button", { name: "Try Again" });
  expect(await screen.findByRole("link", { name: "Still available" })).toBeVisible();
  await fireEvent.click(retry);
  expect(await screen.findByRole("link", { name: "Recovered single" })).toBeVisible();
  expect(requests).toBe(2);
  expect(fetch.mock.calls.filter(([url]) => url.includes("/albums/recent"))).toHaveLength(1);
});
