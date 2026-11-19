import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import RadioPage from "./+page.svelte";

afterEach(() => vi.unstubAllGlobals());

it("offers a retry for a failed genre request and shows the empty state after recovery", async () => {
  setLocale("en");
  vi.stubGlobal("fetch", vi.fn()
    .mockResolvedValueOnce(new Response(null, { status: 503 }))
    .mockResolvedValueOnce(Response.json([])));
  render(RadioPage);
  await screen.findByText("This selection could not be loaded.");
  await fireEvent.click(screen.getByRole("button", { name: "Try Again" }));
  await screen.findByText("No results found");
  expect(screen.queryByRole("button", { name: "Try Again" })).toBeNull();
  expect(screen.getByRole("link", { name: "Sign In" })).toHaveAttribute("href", "/login");
});
