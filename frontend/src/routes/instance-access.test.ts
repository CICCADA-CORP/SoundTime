import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { createRawSnippet } from "svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import Layout from "./+layout.svelte";

const mocks = vi.hoisted(() => ({
  get: vi.fn(), goto: vi.fn(), page: { url: new URL("http://localhost/") },
  auth: { init: vi.fn(), isAuthenticated: false, isAdmin: false },
}));
vi.mock("$app/navigation", () => ({ goto: mocks.goto, afterNavigate: vi.fn() }));
vi.mock("$app/state", () => ({ page: mocks.page }));
vi.mock("$lib/api", async importOriginal => ({
  ...await importOriginal<typeof import("$lib/api")>(), api: { get: mocks.get },
}));
vi.mock("$lib/stores/auth.svelte", () => ({ getAuthStore: () => mocks.auth }));
vi.mock("$lib/stores/theme.svelte", () => ({ getThemeStore: () => ({ init: vi.fn() }) }));
const children = createRawSnippet(() => ({ render: () => "<p>Requested page</p>" }));
const setup = { setup_complete: true, has_admin: true, instance_private: true };

beforeEach(() => {
  vi.resetAllMocks();
  setLocale("en");
  mocks.page.url = new URL("http://localhost/albums/42?view=list#track");
});

it("holds the private page back until the instance status has been checked", async () => {
  let ready!: (value: typeof setup) => void;
  mocks.get.mockImplementation(() => new Promise(resolve => { ready = resolve; }));
  render(Layout, { children });
  expect(screen.getByRole("status")).toBeVisible();
  expect(screen.queryByText("Requested page")).toBeNull();
  ready(setup);
  await waitFor(() => expect(mocks.goto).toHaveBeenCalledWith("/login?next=%2Falbums%2F42%3Fview%3Dlist%23track", { replaceState: true }));
  expect(screen.queryByText("Requested page")).toBeNull();
});

it("offers a local demo on request and checks visibility again on retry", async () => {
  mocks.get.mockRejectedValueOnce(new TypeError("offline")).mockResolvedValueOnce(setup);
  render(Layout, { children });
  expect(await screen.findByRole("heading", { name: "This instance is unavailable" })).toBeVisible();
  expect(screen.queryByText("Requested page")).toBeNull();
  expect(screen.queryByText("Last Light")).toBeNull();
  await fireEvent.click(screen.getByRole("button", { name: "Listen to the offline demo" }));
  expect(screen.getByRole("region", { name: "Offline demo" })).toBeVisible();
  expect(screen.getByText("Last Light")).toBeVisible();
  expect(mocks.get.mock.calls).toEqual([["/setup/status"]]);
  await fireEvent.click(screen.getByRole("button", { name: "Retry connection" }));
  await waitFor(() => expect(mocks.goto).toHaveBeenCalledWith("/login?next=%2Falbums%2F42%3Fview%3Dlist%23track", { replaceState: true }));
});

it("keeps the account form accessible when visibility is unknown and hides instance links", async () => {
  mocks.page.url = new URL("http://localhost/login");
  mocks.get.mockResolvedValue({ setup_complete: true });
  render(Layout, { children });
  expect(await screen.findByText("Requested page")).toBeVisible();
  expect(screen.queryByRole("link", { name: "Home" })).toBeNull();
  expect(mocks.goto).not.toHaveBeenCalled();
});
