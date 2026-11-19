import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { createRawSnippet } from "svelte";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import AppShell from "./AppShell.svelte";
import TaskBanner from "./TaskBanner.svelte";

const page = vi.hoisted(() => ({ url: new URL("https://music.example/albums/a?view=list#two") }));
const auth = vi.hoisted(() => ({ isAuthenticated: false, isAdmin: false, user: null }));
const tasks = vi.hoisted(() => ({
  isActive: true, isRunning: false, taskType: "sync", lastStatus: null as { status: string } | null,
  progress: null as { processed: number; total: number } | null, dismiss: vi.fn(),
}));
vi.mock("$app/state", () => ({ page }));
vi.mock("$app/navigation", () => ({ afterNavigate: vi.fn(), goto: vi.fn() }));
vi.mock("$lib/stores/auth.svelte", () => ({ getAuthStore: () => auth }));
vi.mock("$lib/stores/tasks.svelte", () => ({ getTaskStore: () => tasks }));
const children = createRawSnippet(() => ({ render: () => "<p>Album tracks</p>" }));

beforeEach(() => {
  setLocale("en");
  vi.clearAllMocks();
  Object.assign(tasks, { isActive: true, isRunning: false, taskType: "sync", lastStatus: null, progress: null });
  Object.assign(auth, { isAuthenticated: false, isAdmin: false });
  vi.stubGlobal("fetch", vi.fn(async () => Response.json({ data: [] })));
});
afterEach(() => vi.unstubAllGlobals());
function viewport(desktop: boolean) {
  vi.stubGlobal("matchMedia", vi.fn((query: string) => ({
    matches: query.includes("min-width") && desktop, media: query,
    addEventListener: vi.fn(), removeEventListener: vi.fn(),
  })));
}

it("lets a guest search, open and close the mobile sidebar and sign in back to this album", async () => {
  viewport(false);
  render(AppShell, { children });
  const header = within(screen.getByRole("banner"));
  expect(header.getByRole("link", { name: "Sign In" })).toHaveAttribute("href", "/login?next=%2Falbums%2Fa%3Fview%3Dlist%23two");
  expect(header.queryByRole("link", { name: "Upload" })).toBeNull();
  expect(screen.queryByRole("status")).toBeNull();
  const toggle = header.getByRole("button", { name: "Show navigation" });
  toggle.focus();
  await fireEvent.click(toggle);
  const nav = await screen.findByRole("dialog");
  await fireEvent.keyDown(nav, { key: "Escape" });
  await waitFor(() => expect(toggle).toHaveFocus());
  const search = header.getByRole("button", { name: "Search" });
  await fireEvent.click(search);
  expect(await screen.findByRole("dialog")).toBeVisible();
  await fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  await waitFor(() => expect(search).toHaveFocus());
});

it("puts radio and podcasts directly after Radio on mobile and keeps a settings link when a guest opts out", async () => {
  viewport(false);
  const { webPreferences } = await import("$lib/webradios/preferences.svelte");
  localStorage.removeItem("soundtime_webradios_guest");
  vi.stubGlobal("fetch", vi.fn(async (url: string) => Response.json(url.endsWith("/webradios/status")
    ? { instance_enabled: true, user_enabled: true, enabled: true } : { data: [] })));
  render(AppShell, { children });
  const link = await screen.findByRole("link", { name: "Radio & podcasts" });
  expect(link).toHaveAttribute("href", "/webradios");
  expect(link.previousElementSibling).toHaveAttribute("href", "/radio");
  await webPreferences.setEnabled(false, null);
  await waitFor(() => expect(screen.queryByRole("link", { name: "Radio & podcasts" })).toBeNull());
  await fireEvent.click(screen.getByRole("button", { name: "Show navigation" }));
  const drawer = await screen.findByRole("dialog");
  expect(within(drawer).getByRole("link", { name: "Radio and podcast settings" })).toHaveAttribute("href", "/settings#webradios");
  await webPreferences.setEnabled(true, null);
});

it("lets an administrator collapse the desktop sidebar and inspect an active task", async () => {
  Object.assign(auth, { isAuthenticated: true, isAdmin: true });
  Object.assign(tasks, { isRunning: true, progress: { processed: 4, total: 7 } });
  viewport(true);
  render(AppShell, { children });
  const header = within(screen.getByRole("banner"));
  expect(header.getByRole("link", { name: "Upload" })).toHaveAttribute("href", "/upload");
  expect(header.getByRole("link", { name: "Settings" })).toHaveAttribute("href", "/settings");
  expect(within(screen.getByRole("status")).getByRole("link", { name: /Storage sync in progress.*4 \/ 7/ })).toHaveAttribute("href", "/admin");
  expect(screen.queryByRole("button", { name: "Dismiss" })).toBeNull();
  await fireEvent.click(header.getByRole("button", { name: "Hide navigation" }));
  expect(header.getByRole("button", { name: "Show navigation" })).toHaveAttribute("aria-expanded", "false");
});

it.each([
  ["sync", true, "Storage sync in progress…"],
  ["integrity", true, "Integrity check in progress…"],
  ["sync", false, "Storage sync completed"],
  ["integrity", false, "Integrity check completed"],
])("announces the %s task and offers dismissal only after completion (running %s)", async (taskType, running, label) => {
  Object.assign(tasks, { taskType, isRunning: running, progress: { processed: 5, total: 0 } });
  render(TaskBanner);
  expect(screen.getByRole("status")).toHaveTextContent(label + " · 5");
  const dismiss = screen.queryByRole("button", { name: "Dismiss" });
  if (running) { expect(dismiss).toBeNull(); return; }
  await fireEvent.click(dismiss!);
  expect(tasks.dismiss).toHaveBeenCalledOnce();
});
it("announces a failed task and only dismisses after the admin acts", async () => {
  tasks.lastStatus = { status: "error" };
  render(TaskBanner);
  expect(screen.getByRole("status")).toHaveTextContent("Task error");
  await fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  expect(tasks.dismiss).toHaveBeenCalledOnce();
});
it("hides a dismissed task", () => {
  tasks.isActive = false;
  render(TaskBanner);
  expect(screen.queryByRole("status")).toBeNull();
});
