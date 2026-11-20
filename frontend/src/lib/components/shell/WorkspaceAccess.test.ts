import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { createRawSnippet } from "svelte";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import * as queueStore from "$lib/stores/queue.svelte";
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

function viewport(width: number) {
  vi.stubGlobal("matchMedia", (query: string) => ({
    media: query,
    matches: width >= Number(query.match(/min-width: (\d+)px/)?.[1] || Infinity),
    addEventListener: vi.fn(), removeEventListener: vi.fn(),
  }));
}

beforeEach(() => {
  viewport(390);
  setLocale("en");
  vi.clearAllMocks();
  Object.assign(tasks, { isActive: true, isRunning: false, taskType: "sync", lastStatus: null, progress: null });
  Object.assign(auth, { isAuthenticated: false, isAdmin: false });
  vi.stubGlobal("fetch", vi.fn(async () => Response.json({ data: [] })));
});
afterEach(() => vi.unstubAllGlobals());
it("lets a guest search, open and close navigation and sign in back to this album", async () => {
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
  viewport(390);
  const { webPreferences } = await import("$lib/webradios/preferences.svelte");
  localStorage.removeItem("soundtime_webradios_guest");
  vi.stubGlobal("fetch", vi.fn(async (url: string) => Response.json(url.endsWith("/webradios/status")
    ? { instance_enabled: true, user_enabled: true, enabled: true } : { data: [] })));
  render(AppShell, { children });
  const toggle = screen.getByRole("button", { name: "Show navigation" });
  await fireEvent.click(toggle);
  const drawer = await screen.findByRole("dialog", { name: "Navigation" });
  const menu = within(drawer);
  const link = menu.getByRole("link", { name: "Radio & podcasts" });
  expect(link).toHaveAttribute("href", "/webradios");
  expect(link.previousElementSibling).toBe(menu.getByRole("link", { name: "Radio" }));
  await webPreferences.setEnabled(false, null);
  await waitFor(() => expect(menu.queryByRole("link", { name: "Radio & podcasts" })).toBeNull());
  expect(menu.getByRole("link", { name: "Radio and podcast settings" })).toHaveAttribute("href", "/settings#webradios");
  await webPreferences.setEnabled(true, null);
});

it("lets an administrator use the header, the directory and an active task", async () => {
  Object.assign(auth, { isAuthenticated: true, isAdmin: true });
  Object.assign(tasks, { isRunning: true, progress: { processed: 4, total: 7 } });
  render(AppShell, { children });
  const header = within(screen.getByRole("banner"));
  await fireEvent.click(header.getByRole("button", { name: "Settings" }));
  const account = await screen.findByRole("dialog");
  expect(within(account).getByRole("link", { name: "Settings" })).toHaveAttribute("href", "/settings");
  await fireEvent.keyDown(account, { key: "Escape" });
  expect(within(screen.getByRole("status")).getByRole("link", { name: /Storage sync in progress.*4 \/ 7/ })).toHaveAttribute("href", "/admin");
  expect(screen.queryByRole("button", { name: "Dismiss" })).toBeNull();
  const open = header.getByRole("button", { name: "Show navigation" });
  await fireEvent.click(open);
  const directory = await screen.findByRole("dialog", { name: "Navigation" });
  expect(within(directory).getByRole("link", { name: "Radio" })).toHaveAttribute("href", "/radio");
  expect(within(directory).getByRole("link", { name: "Upload" })).toHaveAttribute("href", "/upload");
  expect(within(directory).getByRole("link", { name: "Admin" })).toHaveAttribute("href", "/admin");
  await fireEvent.keyDown(directory, { key: "Escape" });
  await waitFor(() => expect(open).toHaveFocus());
});

it("opens the queue on a tablet and returns focus to the idle player", async () => {
  viewport(900);
  render(AppShell, { children });
  expect(screen.queryByRole("dialog")).toBeNull();
  const button = within(screen.getByTestId("idle-player")).getByRole("button", { name: "Queue" });
  button.focus();
  await fireEvent.click(button);
  const queue = await screen.findByRole("dialog", { name: "Queue" });
  expect(await within(queue).findByText("Queue is empty")).toBeVisible();
  await fireEvent.keyDown(queue, { key: "Escape" });
  await waitFor(() => expect(button).toHaveFocus());
  expect(button).toHaveAttribute("aria-expanded", "false");
});

it("keeps desktop library links visible and reuses the queue suggestions when reopening the panel", async () => {
  viewport(1440);
  render(AppShell, { children });
  const sidebar = screen.getByRole("navigation", { name: "Navigation" });
  expect(within(sidebar).getByRole("link", { name: "Albums" })).toHaveAttribute("href", "/albums");
  const rail = () => screen.getByRole("complementary", { name: "Queue" });
  const recentRequests = () => vi.mocked(fetch).mock.calls.filter(([url]) => String(url).includes("/tracks/recently-added")).length;
  await waitFor(() => expect(recentRequests()).toBe(1));
  await fireEvent.click(within(rail()).getByRole("button", { name: "Close" }));
  const toggle = within(screen.getByTestId("idle-player")).getByRole("button", { name: "Queue" });
  await waitFor(() => expect(toggle).toHaveFocus());
  await fireEvent.click(toggle);
  await waitFor(() => expect(within(rail()).getByRole("button", { name: "Close" })).toHaveFocus());
  expect(recentRequests()).toBe(1);
});

it("reserves the idle queue and plays the illustrated suggestion with the other recent tracks in order", async ({ onTestFinished }) => {
  viewport(1440);
  const queue = queueStore.getQueueStore();
  const playQueue = vi.fn();
  const spy = vi.spyOn(queueStore, "getQueueStore").mockReturnValue({ ...queue, playQueue });
  onTestFinished(() => spy.mockRestore());
  const tracks = Array.from({ length: 15 }, (_, index) => ({
    id: String(index), title: "Track " + index, duration_secs: 120,
    cover_url: [0, 10, 13].includes(index) ? "/covers/" + index + ".jpg" : null,
    availability: index === 0 ? "withdrawn" : "playable",
  }));
  let provide!: (response: Response) => void;
  const request = new Promise<Response>(resolve => { provide = resolve; });
  vi.mocked(fetch).mockImplementation(async url => String(url).includes("/tracks/recently-added?per_page=24")
    ? request : Response.json({ data: [] }));
  render(AppShell, { children });
  const panel = within(screen.getByRole("complementary", { name: "Queue" }));
  expect(panel.getByRole("heading", { name: "Queue" })).toBeVisible();
  expect(panel.queryByText("Queue is empty")).toBeNull();
  provide(Response.json({ data: tracks }));
  const hero = await panel.findByRole("button", { name: "Play · Track 10" });
  expect(panel.getByText("Listen Now")).toBeVisible();
  const expected = [tracks[10], ...tracks.slice(1, 8)];
  const plays = panel.getAllByRole("button", { name: /^Play · Track / });
  expect(plays.map(button => button.getAttribute("aria-label"))).toEqual(expected.map(track => "Play · " + track.title));
  await fireEvent.click(hero);
  expect(playQueue).toHaveBeenCalledWith(expected, 0, "collection");
  await fireEvent.click(plays[3]);
  expect(playQueue).toHaveBeenCalledWith(expected, 3, "collection");
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
