import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { invalidatePlaylists } from "$lib/catalog/playlists.svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import LibraryNavigation from "./LibraryNavigation.svelte";

const get = vi.hoisted(() => vi.fn());
const auth = vi.hoisted(() => ({ isAuthenticated: false, isAdmin: false }));
const location = vi.hoisted(() => ({ url: new URL("http://localhost/") }));
vi.mock("$lib/api", () => ({ api: { get } }));
vi.mock("$lib/stores/auth.svelte", () => ({ getAuthStore: () => auth }));
vi.mock("$app/state", () => ({ page: location }));

beforeEach(() => {
  setLocale("en");
  get.mockReset();
  auth.isAuthenticated = auth.isAdmin = false;
  location.url = new URL("http://localhost/");
});

it("links to four recent public playlists and marks only the current one", async () => {
  location.url = new URL("http://localhost/playlists/two");
  const record = (id: string, name: string) => ({ id, name, is_public: true });
  get.mockResolvedValue({ data: [
    null, { ...record("private", "Private mix"), is_public: false },
    { ...record("one", "Songbook"), cover_url: "/covers/songbook.jpg" },
    record("one", "Duplicate"), record("two", "On repeat"), record("three", "Sunday"),
    record("four", "Favorites collection"), record("five", "Older collection"),
  ] });
  const close = vi.fn();
  const { container } = render(LibraryNavigation, { props: { close } });

  const current = await screen.findByRole("link", { name: "On repeat" });
  expect(get).toHaveBeenCalledWith("/playlists?per_page=4");
  expect(current).toHaveAttribute("aria-current", "page");
  expect(screen.getByRole("link", { name: "Playlists" })).not.toHaveAttribute("aria-current");
  expect(container.querySelectorAll("[aria-current]")).toHaveLength(1);
  expect(screen.getByRole("region", {name: "Recent playlists"}).querySelectorAll("a")).toHaveLength(4);
  expect(screen.getByRole("link", {name:"Songbook"}).querySelector("img")).toHaveAttribute("src","/covers/songbook.jpg");
  for (const name of ["Private mix", "Duplicate", "Older collection", "Settings", "History"]) {
    expect(screen.queryByRole("link", { name })).toBeNull();
  }
  current.addEventListener("click", (event) => event.preventDefault());
  await fireEvent.click(current, { ctrlKey: true });
  expect(close).not.toHaveBeenCalled();
  await fireEvent.click(current);
  expect(close).toHaveBeenCalledOnce();
});

it.each([false, true])("keeps catalog and authorized destinations available through a failed request (admin %s)", async (isAdmin) => {
  auth.isAuthenticated = true;
  auth.isAdmin = isAdmin;
  location.url = new URL("http://localhost/playlists/unlisted");
  get.mockRejectedValue(new Error("Network unavailable"));
  render(LibraryNavigation, { props: { close: vi.fn() } });
  await waitFor(() => expect(get).toHaveBeenCalledOnce());
  expect(screen.queryByRole("region", {name:"Recent playlists"})).toBeNull();
  expect(screen.getByRole("link", {name:"Playlists"})).toHaveAttribute("aria-current","page");
  expect(screen.getByRole("link", {name:"History"})).toHaveAttribute("href","/history");
  expect(screen.getByRole("link", {name:"Upload"})).toHaveAttribute("href","/upload");
  expect(!!screen.queryByRole("link", {name:"Admin"})).toBe(isAdmin);
});

it("refreshes recent links after a playlist changes while the sidebar stays open", async () => {
  const playlist = { id: "mix", name: "Morning", is_public: true };
  get.mockResolvedValueOnce({ data: [playlist] }).mockResolvedValueOnce({ data: [{ ...playlist, name: "Evening" }] });
  render(LibraryNavigation);
  await screen.findByRole("link", { name: "Morning" });
  invalidatePlaylists();
  expect(await screen.findByRole("link", { name: "Evening" })).toHaveAttribute("href", "/playlists/mix");
  expect(screen.queryByRole("link", { name: "Morning" })).toBeNull();
});
