import { afterEach, expect, it, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import Albums from "./+page.svelte";

afterEach(() => vi.unstubAllGlobals());

it("loads the next album shelf and returns to the previous one", async () => {
  setLocale("en");
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    const page = new URL(url, "http://localhost").searchParams.get("page");
    return Response.json({ data: [{ id: page, title: `Record ${page}` }], total_pages: 2 });
  }));
  render(Albums);
  expect(await screen.findByRole("link", { name: /Record 1/ })).toHaveAttribute("href", "/albums/1");
  expect(screen.getByRole("button", { name: "Previous" })).toBeDisabled();
  await fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(await screen.findByRole("link", { name: /Record 2/ })).toHaveAttribute("href", "/albums/2");
  expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
  await fireEvent.click(screen.getByRole("button", { name: "Previous" }));
  expect(await screen.findByRole("link", { name: /Record 1/ })).toBeVisible();
});
