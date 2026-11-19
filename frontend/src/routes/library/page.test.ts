import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { goto } from "$app/navigation";
import { setLocale } from "$lib/i18n/index.svelte";
import Library from "./+page.svelte";

vi.mock("$lib/stores/auth.svelte", () => ({ getAuthStore: () => ({ user: { id: "member" }, isAuthenticated: true }) }));
afterEach(() => vi.unstubAllGlobals());

it("loads the 101st personal upload from its page link and resets pagination when switching collections", async () => {
  setLocale("en");
  await goto("/library?tab=uploads&page=2");
  const fetch = vi.fn(async (url: string) => {
    const { pathname, searchParams } = new URL(url, "http://localhost");
    if (pathname === "/api/tracks/my-uploads") {
      const id = searchParams.get("page") === "3" ? "101" : "51";
      return Response.json({ data: [{ id, title: "Import " + id, duration_secs: 3 }], total_pages: 3, total: 101 });
    }
    if (pathname === "/api/me/playlists") {
      return Response.json({ data: [{ id: "private", name: "My private list", is_public: false }], total_pages: 1 });
    }
    return Response.json([]);
  });
  vi.stubGlobal("fetch", fetch);
  render(Library);
  expect(await screen.findByRole("link", { name: "Import 51" })).toBeVisible();
  await fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(await screen.findByRole("link", { name: "Import 101" })).toHaveAttribute("href", "/tracks/101");
  expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
  await fireEvent.click(screen.getByRole("tab", { name: "My playlists" }));
  expect(await screen.findByRole("link", { name: "My private list" })).toHaveAttribute("href", "/playlists/private");
  expect(fetch.mock.calls.some(([url]) => url === "/api/me/playlists?page=1&per_page=20")).toBe(true);
  expect(fetch.mock.calls.some(([url]) => url.startsWith("/api/albums"))).toBe(false);
});
