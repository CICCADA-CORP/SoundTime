import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import { getPlayerStore } from "$lib/stores/player.svelte";
import { getQueueStore } from "$lib/stores/queue.svelte";
import Settings from "./+page.svelte";

const auth = vi.hoisted(() => ({
  user: { username: "alice", instance_id: "alice@example.test", role: "user", email: "alice@example.test" },
  isAuthenticated: true, updateEmail: vi.fn(), updatePassword: vi.fn(), deleteAccount: vi.fn(), logout: vi.fn(),
}));
const lastfm = vi.hoisted(() => ({ status: vi.fn(), disconnect: vi.fn(), toggleScrobble: vi.fn(), callback: vi.fn(), connect: vi.fn() }));
vi.mock("$lib/stores/auth.svelte", () => ({ getAuthStore: () => auth }));
vi.mock("$lib/api", async original => ({
  ...await original<typeof import("$lib/api")>(),
  lastfmApi: lastfm,
}));

beforeEach(() => {
  vi.clearAllMocks();
  lastfm.status.mockResolvedValue({ connected: false });
  setLocale("en");
  auth.isAuthenticated = true;
});

it("offers sign in to a visitor opening settings directly", () => {
  auth.isAuthenticated = false;
  render(Settings);
  expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/login?next=%2Fsettings");
  expect(screen.queryByText("alice@example.test")).toBeNull();
  expect(screen.queryByRole("button", { name: "Delete my account" })).toBeNull();
});

it("controls the same volume and autoplay preferences used by the player", async () => {
  render(Settings);
  const slider = screen.getByRole("slider", { name: "Volume" });
  await fireEvent.input(slider, { target: { value: "34" } });
  expect(getPlayerStore().volume).toBe(0.34);
  expect(localStorage.getItem("soundtime_volume")).toBe("0.34");
  expect(slider).toHaveAttribute("aria-valuetext", "34%");
  const before = getQueueStore().autoplay;
  await fireEvent.click(screen.getByRole("switch", { name: "Autoplay" }));
  expect(getQueueStore().autoplay).toBe(!before);
  expect(localStorage.getItem("soundtime_autoplay")).toBe(String(!before));
  await fireEvent.change(screen.getByRole("combobox", { name: "Display Language" }), { target: { value: "fr" } });
  expect(localStorage.getItem("soundtime_lang")).toBe("fr");
});

it("lets a listener update their email, cancel account deletion and log out", async () => {
  render(Settings);
  const email = screen.getByLabelText("New email address");
  await fireEvent.input(email, { target: { value: "other@example.test" } });
  await fireEvent.input(screen.getByLabelText("Confirm with your password"), { target: { value: "passphrase" } });
  await fireEvent.submit(email.closest("form")!);
  expect(await screen.findByRole("status")).toHaveTextContent("Email address updated successfully.");
  expect(auth.updateEmail).toHaveBeenCalledWith("other@example.test", "passphrase");
  const deleteTrigger = screen.getByRole("button", { name: "Delete my account" });
  deleteTrigger.focus();
  await fireEvent.click(deleteTrigger);
  const dialog = await screen.findByRole("dialog");
  await fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await waitFor(() => expect(deleteTrigger).toHaveFocus());
  expect(auth.deleteAccount).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole("button", { name: "Logout" }));
  expect(auth.logout).toHaveBeenCalledOnce();
});

it("validates confirmation and allows a corrected password change after an API error", async () => {
  render(Settings);
  const current = screen.getByLabelText("Current password");
  const next = screen.getByLabelText("New password");
  const confirm = screen.getByLabelText("Confirm Password");
  await fireEvent.input(current, { target: { value: "first-password" } });
  await fireEvent.input(next, { target: { value: "second-password" } });
  await fireEvent.input(confirm, { target: { value: "typo-password" } });
  await fireEvent.submit(current.closest("form")!);
  expect(await screen.findByRole("alert")).toHaveTextContent("Passwords do not match.");
  expect(auth.updatePassword).not.toHaveBeenCalled();
  await fireEvent.input(confirm, { target: { value: "second-password" } });
  auth.updatePassword.mockRejectedValueOnce(new Error("Server unavailable"));
  await fireEvent.submit(current.closest("form")!);
  expect(await screen.findByRole("alert")).toHaveTextContent("Server unavailable");
  await fireEvent.submit(current.closest("form")!);
  expect(await screen.findByRole("status")).toHaveTextContent("Password changed successfully.");
  expect(current).toHaveValue("");
});

it("keeps a failed Last.fm change available for retry and disconnects through its API", async () => {
  lastfm.status.mockResolvedValueOnce({ connected: true, username: "listener", scrobble_enabled: false });
  lastfm.toggleScrobble.mockRejectedValueOnce(new Error("Last.fm offline"));
  render(Settings);
  const toggle = await screen.findByRole("switch", { name: "Scrobbling" });
  await fireEvent.click(toggle);
  expect(await screen.findByRole("alert")).toHaveTextContent("Last.fm offline");
  expect(toggle).toHaveAttribute("aria-checked", "false");
  await fireEvent.click(toggle);
  await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
  expect(screen.queryByRole("alert")).toBeNull();
  await fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
  expect(await screen.findByRole("button", { name: "Connect Last.fm" })).toBeVisible();
  expect(lastfm.disconnect).toHaveBeenCalledOnce();
});
