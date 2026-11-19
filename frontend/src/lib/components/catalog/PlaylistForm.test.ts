import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import type { Playlist } from "$lib/types";
import PlaylistForm from "./PlaylistForm.svelte";

const record: Playlist = {
  id: "jazz", name: "Jazz", description: null, is_public: true, owner_id: "member",
  cover_url: null, created_at: "2025-01-01", updated_at: "2025-01-01"
};
afterEach(() => vi.unstubAllGlobals());

it("creates a private playlist with trimmed fields and returns the server record", async () => {
  setLocale("en");
  const saved = { ...record, is_public: false };
  const fetch = vi.fn().mockResolvedValue(Response.json(saved));
  vi.stubGlobal("fetch", fetch);
  const onsave = vi.fn();
  render(PlaylistForm, { onsave, oncancel: vi.fn() });
  const create = screen.getByRole("button", { name: "Create" });
  expect(create).toBeDisabled();
  await fireEvent.input(screen.getByLabelText("Name"), { target: { value: " Jazz " } });
  await fireEvent.click(screen.getByLabelText("Public playlist"));
  await fireEvent.click(create);
  await waitFor(() => expect(onsave).toHaveBeenCalledWith(saved));
  expect(fetch).toHaveBeenCalledWith("/api/playlists", expect.objectContaining({
    method: "POST", body: JSON.stringify({ name: "Jazz", description: "", is_public: false })
  }));
});

it("keeps edits after a failed save and can retry or cancel", async () => {
  setLocale("en");
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json({ error: "Unable to save playlist" }, { status: 503 }))
    .mockResolvedValueOnce(Response.json(record));
  vi.stubGlobal("fetch", fetch);
  const onsave = vi.fn(), oncancel = vi.fn();
  render(PlaylistForm, { playlist: record, onsave, oncancel });
  const cover = screen.getByLabelText("Cover URL");
  await fireEvent.input(cover, { target: { value: "https://example.test/jazz.png" } });
  const save = screen.getByRole("button", { name: "Save" });
  await fireEvent.click(save);
  expect(await screen.findByRole("alert")).toHaveTextContent("Unable to save playlist");
  expect(cover).toHaveValue("https://example.test/jazz.png");
  await fireEvent.click(save);
  await waitFor(() => expect(onsave).toHaveBeenCalledWith(record));
  expect(fetch).toHaveBeenLastCalledWith("/api/playlists/jazz", expect.objectContaining({
    method: "PUT", body: JSON.stringify({ name: "Jazz", description: "", is_public: true, cover_url: "https://example.test/jazz.png" })
  }));
  await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(oncancel).toHaveBeenCalledOnce();
});
