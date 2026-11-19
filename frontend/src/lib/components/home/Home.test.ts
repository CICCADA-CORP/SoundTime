import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import { showcaseTracks } from "$lib/showcase";
import Home from "../../../routes/+page.svelte";

const playQueue = vi.fn();
vi.mock("$lib/stores/queue.svelte", () => ({ getQueueStore: () => ({ playQueue }) }));

function emptyResponse(path: string) {
  if (path.includes("/stats/overview")) return { total_tracks: 0 };
  if (path.includes("/random") || path.includes("/editorial-playlists")) return [];
  return { data: [] };
}

describe("home catalogue", () => {
  afterEach(() => vi.unstubAllGlobals());

  beforeEach(() => {
    setLocale("en");
    playQueue.mockClear();
  });

  it("plays the local preview during an outage, then displays a recovered empty catalogue", async () => {
    let offline = true;
    vi.stubGlobal("fetch", vi.fn(async (url: string) => offline
      ? new Response(null, { status: 503 })
      : Response.json(emptyResponse(url))));
    render(Home);
    expect(screen.queryByRole("region", { name: "After Hours" })).toBeNull();
    await fireEvent.click(await screen.findByRole("button", { name: "Listen to the offline demo" }));
    await screen.findByRole("region", { name: "After Hours" });
    await fireEvent.click(screen.getByRole("button", { name: "Listen now" }));
    expect(playQueue).toHaveBeenCalledWith(showcaseTracks, 0, "collection");

    offline = false;
    await fireEvent.click(screen.getByRole("button", { name: /Retry connection/ }));
    const upload = await screen.findByRole("link", { name: "Sign in to upload music." });
    expect(upload).toHaveAttribute("href", "/login?next=%2Fupload");
    expect(screen.queryByRole("region", { name: "After Hours" })).toBeNull();
  });

  it("keeps available tracks playable when another shelf fails", async () => {
    const track = { ...showcaseTracks[0], id: "catalogue-track", title: "Live catalogue track" };
    vi.stubGlobal("fetch", vi.fn(async (url: string) => url.includes("/albums/recent")
      ? new Response(null, { status: 502 })
      : Response.json(url.includes("/tracks/random") ? [track] : emptyResponse(url))));
    render(Home);
    await screen.findByText("Some sections couldn’t load.");
    await fireEvent.click(screen.getByRole("button", { name: "List view" }));
    await fireEvent.click(screen.getByRole("button", { name: /Live catalogue track/ }));
    expect(playQueue).toHaveBeenCalledWith([track], 0, "explore");
    expect(screen.queryByRole("region", { name: "After Hours" })).toBeNull();
  });

  it("filters the preview, sorts the playback queue and browses its album and artist offline", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(null, { status: 503 })));
    render(Home);
    await fireEvent.click(await screen.findByRole("button", { name: "Listen to the offline demo" }));
    await screen.findByRole("region", { name: "After Hours" });
    const tracks = within(screen.getByRole("tabpanel", { name: "Tracks" }));
    await fireEvent.input(tracks.getByRole("searchbox"), { target: { value: "last" } });
    await fireEvent.click(tracks.getByRole("button", { name: /Last Light/ }));
    expect(playQueue).toHaveBeenLastCalledWith([showcaseTracks[0]], 0, "explore");
    await fireEvent.change(tracks.getByRole("combobox", { name: "Genre" }), { target: { value: "Ambient" } });
    await fireEvent.click(tracks.getByRole("button", { name: "Clear filters" }));
    await fireEvent.change(tracks.getByRole("combobox", { name: "Sort by" }), { target: { value: "name" } });
    await fireEvent.click(tracks.getByRole("button", { name: /Blue Fold/ }));
    const alphabetic = [showcaseTracks[1], showcaseTracks[3], showcaseTracks[0], showcaseTracks[2]];
    expect(playQueue).toHaveBeenLastCalledWith(alphabetic, 0, "explore");
    for (const [tab, title] of [["Albums", "After Hours"], ["Artists", "SoundTime Studio"]]) {
      await fireEvent.click(screen.getByRole("tab", { name: tab }));
      await fireEvent.click(within(screen.getByRole("tabpanel", { name: tab })).getByRole("button", { name: new RegExp(title) }));
      expect(playQueue).toHaveBeenLastCalledWith(showcaseTracks, 0, "collection");
    }
  });
});
