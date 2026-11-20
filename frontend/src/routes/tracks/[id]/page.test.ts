import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import { showcaseTracks } from "$lib/showcase";
import TrackPage from "./+page.svelte";

const playQueue = vi.hoisted(() => vi.fn());
vi.mock("$lib/stores/queue.svelte", () => ({ getQueueStore: () => ({ playQueue }) }));
vi.mock("$app/state", () => ({ page: { params: { id: "demo" } } }));

const track = {
  ...showcaseTracks[0], id: "demo", title: "Studio take", artist_id: "uncredited", artist_name: null,
  album_id: "session", album_title: "Session tapes", created_at: "2026-03-01",
};
beforeEach(() => setLocale("en"));
afterEach(() => vi.unstubAllGlobals());

it("keeps playback and album navigation available while credits load, fail and retry", async () => {
  let resolveCredits!: (response: Response) => void;
  const credits = vi.fn()
    .mockImplementationOnce(() => new Promise<Response>(resolve => { resolveCredits = resolve; }))
    .mockResolvedValueOnce(Response.json({ ...track, album_year: 2026 }));
  const fetchTrack = vi.fn(() => Response.json(track));
  vi.stubGlobal("fetch", vi.fn((url: string) => url.endsWith("/credits") ? credits() : fetchTrack()));
  const { container } = render(TrackPage);

  expect(await screen.findByRole("heading", { name: track.title })).toBeVisible();
  expect(screen.getByRole("link", { name: track.album_title })).toHaveAttribute("href", "/albums/session");
  const play = screen.getByRole("button", { name: /^play$/i });
  await fireEvent.click(play);
  expect(playQueue).toHaveBeenCalledWith([track], 0, "collection");
  resolveCredits(Response.json({ error: "Unavailable" }, { status: 503 }));
  const retry = await screen.findByRole("button", { name: "Try Again" });
  expect(play).toBeEnabled();
  await fireEvent.click(retry);
  expect(await screen.findByText("2026")).toBeVisible();
  expect(container.querySelector('a[href^="/artists/"]')).toBeNull();
  expect(fetchTrack).toHaveBeenCalledTimes(1);
});

it("keeps the full page in its error state when the track is missing even if credits arrive", async () => {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => url.endsWith("/credits")
    ? Response.json(track) : Response.json({ error: "Not found" }, { status: 404 })));
  render(TrackPage);
  expect(await screen.findByText("This item could not be found.")).toBeVisible();
  expect(screen.queryByRole("heading", { name: track.title })).toBeNull();
  expect(screen.queryByRole("button", { name: /^play$/i })).toBeNull();
});

it("cancels outstanding reads when the listener leaves the track", async () => {
  const signals: AbortSignal[] = [];
  vi.stubGlobal("fetch", vi.fn((_url: string, options: RequestInit) => {
    const signal = options.signal!;
    signals.push(signal);
    return new Promise<Response>((_, reject) => {
      signal.addEventListener("abort", () => reject(signal.reason), { once: true });
    });
  }));
  const { unmount } = render(TrackPage);
  await waitFor(() => expect(signals).toHaveLength(2));
  expect(signals.every(signal => !signal.aborted)).toBe(true);
  await unmount();
  expect(signals.every(signal => signal.aborted)).toBe(true);
});
