import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import SideNavigation from "./SideNavigation.svelte";

const auth = vi.hoisted(() => ({
  isAuthenticated: false,
  user: null as { username: string; email: string; display_name?: string } | null,
}));
const location = vi.hoisted(() => ({ url: new URL("http://localhost/") }));
vi.mock("$lib/stores/auth.svelte", () => ({ getAuthStore: () => auth }));
vi.mock("$app/state", () => ({ page: location }));
vi.mock("$lib/api", () => ({ api: { get: vi.fn().mockResolvedValue({ data: [] }) } }));

beforeEach(() => {
  setLocale("en");
  auth.isAuthenticated = false;
  auth.user = null;
  location.url = new URL("http://localhost/");
});

it("shows direct links to listening and library destinations", () => {
  render(SideNavigation);
  expect(screen.getByRole("link", { name: "Home" })).toHaveAttribute("aria-current", "page");
  expect(screen.getByRole("link", { name: "Explore" })).toHaveAttribute("href", "/explore");
  expect(screen.getByRole("link", { name: "Radio" })).toHaveAttribute("href", "/radio");
  expect(screen.getByRole("link", { name: "Library" })).toHaveAttribute("href", "/library");
  expect(screen.getByRole("link", { name: "Sign In" })).toHaveAttribute("href", "/login");
  expect(screen.queryByRole("button", { name: "Close" })).toBeNull();
});

it("marks the album destination on an album page and closes the mobile drawer", async () => {
  location.url = new URL("http://localhost/albums/example");
  const close = vi.fn();
  render(SideNavigation, { props: { mobile: true, close } });
  const current = screen.getByRole("link", { name: "Albums" });
  expect(current).toHaveAttribute("aria-current", "page");
  current.addEventListener("click", (event) => event.preventDefault());
  await fireEvent.click(current, { metaKey: true });
  expect(close).not.toHaveBeenCalled();
  await fireEvent.click(current);
  expect(close).toHaveBeenCalledOnce();
  await fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(close).toHaveBeenCalledTimes(2);
});

it("links the current account to settings and shows the signed-in identity", () => {
  auth.isAuthenticated = true;
  auth.user = { username: "camille", email: "camille@example.org", display_name: "Camille" };
  location.url = new URL("http://localhost/settings");
  render(SideNavigation);
  const account = screen.getByRole("link", { name: "Settings" });
  expect(account).toHaveAttribute("href", "/settings");
  expect(account).toHaveAttribute("aria-current", "page");
  expect(account).toHaveTextContent("Camille");
  expect(account).toHaveTextContent("camille@example.org");
  expect(screen.queryByRole("link", { name: "Sign In" })).toBeNull();
});
