import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import Setup from "./+page.svelte";

const mocks = vi.hoisted(() => ({
  auth: { isAdmin: false, user: null, init: vi.fn() }, get: vi.fn(), post: vi.fn(),
}));
vi.mock("$lib/stores/auth.svelte", () => ({ getAuthStore: () => mocks.auth }));
vi.mock("$lib/api", () => ({ api: { get: mocks.get, post: mocks.post }, setTokens: vi.fn() }));
const partial = { setup_complete: false, instance_private: false, has_admin: true };
beforeEach(() => { vi.clearAllMocks(); setLocale("en"); mocks.auth.isAdmin = false; });

it("offers sign in to an existing admin after an interrupted setup", async () => {
  mocks.get.mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce(partial);
  render(Setup);
  await fireEvent.click(await screen.findByRole("button", { name: "Retry connection" }));
  expect(await screen.findByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/login?next=%2Fsetup");
  expect(screen.queryByRole("textbox", { name: "Instance name" })).toBeNull();
});

it("lets the existing admin complete a private instance with registrations disabled", async () => {
  mocks.auth.isAdmin = true;
  mocks.get.mockResolvedValue(partial);
  render(Setup);
  const name = await screen.findByLabelText("Instance name");
  await fireEvent.submit(name.closest("form")!);
  await fireEvent.click(await screen.findByRole("checkbox", { name: /Private Instance/ }));
  expect(screen.getByRole("checkbox", { name: "Open registrations" })).toBeDisabled();
  await fireEvent.click(screen.getByRole("button", { name: "Finish setup" }));
  await waitFor(() => expect(mocks.post).toHaveBeenCalledWith("/setup/complete", expect.objectContaining({ instance_private: true })));
});
