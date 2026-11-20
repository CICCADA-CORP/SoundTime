import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import Terms from "./+page.svelte";

afterEach(() => vi.unstubAllGlobals());

it("recovers the published terms and renders the instance's Markdown as text and headings", async () => {
  setLocale("en");
  vi.stubGlobal("fetch", vi.fn().mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValueOnce(Response.json({ content: "# Pilot terms\n\n## Library\n- Keep **your** licence.\n- <soundtime-test>example</soundtime-test>", is_default: false })));
  render(Terms);
  await fireEvent.click(await screen.findByRole("button", { name: /try again/i }));
  expect(await screen.findByRole("heading", { name: "Pilot terms", level: 1 })).toBeVisible();
  expect(screen.getByRole("heading", { name: "Library", level: 2 })).toBeVisible();
  expect(screen.getAllByRole("listitem")).toHaveLength(2);
  expect(screen.getByText("your").tagName).toBe("STRONG");
  expect(document.querySelector("soundtime-test")).toBeNull();
  expect(screen.getByText("<soundtime-test>example</soundtime-test>")).toBeVisible();
});
