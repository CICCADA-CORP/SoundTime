import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { tick } from "svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import SearchDialog from "./SearchDialog.svelte";

const fetchSearch = vi.hoisted(() => vi.fn());
const goto = vi.hoisted(() => vi.fn());
vi.mock("$lib/api", () => ({ apiFetch: fetchSearch }));
vi.mock("$app/navigation", () => ({ goto }));

const empty = { tracks: [], albums: [], artists: [] };
const track = (id: string, title: string) => ({ id, title, artist_name: "The Trio" });

describe("global search dialog", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
    fetchSearch.mockResolvedValue(empty);
    goto.mockResolvedValue(undefined);
  });

  it.each(["metaKey", "ctrlKey"])("opens via %s+K, focuses search and restores focus on Escape", async (modifier) => {
    const origin = document.createElement("button");
    document.body.appendChild(origin);
    try {
      origin.focus();
      render(SearchDialog);
      await fireEvent.keyDown(window, { key: "k", [modifier]: true });
      const input = await screen.findByRole("searchbox", { name: "Search" });
      await waitFor(() => expect(input).toHaveFocus());
      expect(fetchSearch).not.toHaveBeenCalled();
      await fireEvent.keyDown(input, { key: "Escape" });
      await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
      await waitFor(() => expect(origin).toHaveFocus());
    } finally {
      origin.remove();
    }
  });

  it("submits the trimmed and encoded query to the full search even before suggestions arrive", async () => {
    render(SearchDialog, { props: { open: true } });
    const input = await screen.findByRole("searchbox");
    await fireEvent.input(input, { target: { value: "  AC/DC & jazz  " } });
    const all = screen.getByRole("link", { name: "See all results" });
    expect(all).toHaveAttribute("href", "/search?q=AC%2FDC%20%26%20jazz");
    await fireEvent.submit(input.closest("form")!);
    expect(goto).toHaveBeenCalledWith("/search?q=AC%2FDC%20%26%20jazz");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("debounces input, ignores an obsolete response, and opens a suggestion with its real route", async () => {
    let resolveOld!: (value: unknown) => void;
    fetchSearch.mockImplementationOnce(() => new Promise(resolve => { resolveOld = resolve; }));
    fetchSearch.mockResolvedValueOnce({ ...empty, tracks: [track("t2", "New music")] });
    render(SearchDialog, { props: { open: true } });
    const input = await screen.findByRole("searchbox");
    await fireEvent.input(input, { target: { value: "a" } });
    await fireEvent.input(input, { target: { value: "ab" } });
    await waitFor(() => expect(fetchSearch).toHaveBeenCalledTimes(1));
    expect(fetchSearch.mock.calls[0][0]).toBe("/search?q=ab&per_page=2");
    await fireEvent.input(input, { target: { value: "abc" } });
    const latest = await screen.findByRole("link", { name: /New music/ });
    resolveOld({ ...empty, tracks: [track("t1", "Old music")] });
    await tick();
    expect(fetchSearch.mock.calls[0][1].signal.aborted).toBe(true);
    expect(screen.queryByRole("link", { name: /Old music/ })).toBeNull();
    await fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(latest).toHaveFocus();
    await fireEvent.click(latest);
    expect(goto).toHaveBeenCalledWith("/tracks/t2");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("lets the user open a full search when local suggestions fail", async () => {
    fetchSearch.mockRejectedValue(new Error("offline"));
    render(SearchDialog, { props: { open: true } });
    const input = await screen.findByRole("searchbox");
    await fireEvent.input(input, { target: { value: "Bill Evans" } });
    await screen.findByText("Search suggestions are unavailable.");
    await fireEvent.click(screen.getByRole("link", { name: "See all results" }));
    expect(goto).toHaveBeenCalledWith("/search?q=Bill%20Evans");
  });
});
