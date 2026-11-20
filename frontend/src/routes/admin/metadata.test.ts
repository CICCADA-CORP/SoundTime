import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import AdminPage from "./+page.svelte";

vi.mock("$lib/stores/auth.svelte", () => ({
  getAuthStore: () => ({ isAdmin: true, loading: false }),
}));

afterEach(() => vi.unstubAllGlobals());

it("requires an explicit recording choice, preserves it on failure and displays the applied result", async () => {
  setLocale("en");
  const track = "11000000-0000-4000-8000-000000000001";
  const endpoint = `/api/admin/metadata/enrich/${track}`;
  const candidates = Array.from({ length: 6 }, (_, index) => ({
    recording_mbid: `22000000-0000-4000-8000-00000000000${index}`,
    title: "Waves",
    artist_name: "One Artist",
    album_title: `Edition ${index}`,
    duration_ms: 127000,
    disambiguation: `Mix ${index}`,
  }));
  const chosen = `${endpoint}?recording_id=${candidates[2].recording_mbid}`;
  let confirmed = false;
  const requests = vi.fn(async (url: string, options: RequestInit) => {
    if (url === "/api/admin/settings") return Response.json([]);
    if (url === "/api/admin/metadata/task-status") return Response.json({
      status: "completed", result: { total_processed: 30, enriched: 10, ambiguous: 17, not_found: 3, errors: 0 },
    });
    if (url.startsWith("/api/admin/tracks/browse?")) {
      expect(new URLSearchParams(url.split("?")[1]).get("search")).toBe("Waves");
      return Response.json({ data: [{ id: track, title: "Waves", artist_name: "One Artist" }] });
    }
    if (url === endpoint) return Response.json({ track_id: track, status: "ambiguous", candidates });
    if (url === chosen) {
      if (!confirmed) {
        confirmed = true;
        return Response.json({ error: "Try this recording again" }, { status: 503 });
      }
      return Response.json({ track_id: track, status: "enriched", corrected_title: "Waves (live)" });
    }
    return Response.json(null);
  });
  vi.stubGlobal("fetch", requests);
  render(AdminPage);
  await fireEvent.click(screen.getByRole("button", { name: "Metadata" }));
  const search = await screen.findByRole<HTMLInputElement>("searchbox", { name: "Find a track by title" });
  expect(screen.getByText("17")).toBeTruthy();
  await fireEvent.input(search, { target: { value: "Waves" } });
  await fireEvent.submit(search.form!);
  await fireEvent.click(await screen.findByRole("button", { name: "Review Waves · One Artist" }));

  const group = await screen.findByRole("group", { name: "Choose a MusicBrainz recording for Waves" });
  await waitFor(() => expect(group.hasAttribute("disabled")).toBe(false));
  const options = within(group).getAllByRole<HTMLInputElement>("radio");
  expect(options).toHaveLength(5);
  expect(options.every(option => !option.checked)).toBe(true);
  const apply = within(group).getByRole<HTMLButtonElement>("button", { name: "Apply this recording" });
  expect(apply.disabled).toBe(true);
  await fireEvent.click(within(group).getByRole("radio", { name: /One Artist · Edition 2 · 2:07 · Mix 2/ }));
  expect(requests.mock.calls.filter(([, options]) => options.method === "POST").map(([url]) => url)).toEqual([endpoint]);
  await waitFor(() => expect(apply.disabled).toBe(false));
  await fireEvent.click(apply);
  expect((await screen.findByRole("alert")).textContent).toContain("Try this recording again");
  expect(options[2].checked).toBe(true);
  await fireEvent.click(apply);
  const result = await screen.findByRole("rowheader", { name: "Waves (live)" });
  await waitFor(() => expect(document.activeElement).toBe(result));
  expect(screen.queryByRole("radio")).toBeNull();
  expect(requests.mock.calls.filter(([, options]) => options.method === "POST").map(([url]) => url))
    .toEqual([endpoint, chosen, chosen]);
});
