import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { page } from "$app/state";
import { setLocale } from "$lib/i18n/index.svelte";
import Stations from "./+page.svelte";

const { playStation } = vi.hoisted(() => ({ playStation: vi.fn() }));
vi.mock("$lib/stores/player.svelte", () => ({ getPlayerStore: () => ({ playStation }) }));

const station = { id: "77000000-0000-4000-8000-000000000001", name: "Wave FM", icon_url: "https://wave.test/icon.png",
  description: "Independent jazz station", tags: ["jazz", "swing"], country: "FR", language: "French", codec: "AAC", bitrate: 128, hls: false };
const enabled = { instance_enabled: true, user_enabled: true, enabled: true };

beforeEach(() => {
  window.dispatchEvent(new Event("soundtime:logout"));
  localStorage.clear();
  setLocale("en");
});
afterEach(() => vi.unstubAllGlobals());

it("renders deep-linked filters, the station details and genre/pagination links, without autoplay", async () => {
  Object.assign(page, {url: new URL("https://app.test/webradios?name=Wave&tag=jazz&offset=30")});
  const fetchMock = vi.fn(async (url: string) => {
    if (url.endsWith("/status")) return Response.json(enabled);
    const search = new URL(url, "https://app.test").searchParams;
    expect([...search.entries()]).toEqual([["name","Wave"],["tag","jazz"],["offset","30"],["limit","30"]]);
    return Response.json({ stations: [station, {
      ...station, id:"77000000-0000-4000-8000-000000000002", name:"Jazz Two", description:"FR · French · AAC · 128 kbps", tags:[]
    }], next_offset: 60 });
  });
  vi.stubGlobal("fetch",fetchMock);
  render(Stations);
  expect(await screen.findByRole("heading", {name:"Wave FM"})).toBeTruthy();
  expect(screen.getByText("Independent jazz station")).toBeTruthy();
  const second = screen.getByRole("heading",{name:"Jazz Two"}).closest("article")!;
  expect(second.textContent?.match(/128 kbps/g)).toHaveLength(1);
  expect(second.textContent?.match(/French/g)).toHaveLength(1);
  expect((screen.getByRole("searchbox", {name:"Station name"}) as HTMLInputElement).value).toBe("Wave");
  expect(screen.getByRole("link",{name:"swing"})).toHaveAttribute("href","/webradios?name=Wave&tag=swing");
  expect(screen.getByRole("link",{name:"Next"})).toHaveAttribute("href","/webradios?name=Wave&tag=jazz&offset=60");
  expect(screen.getByRole("link",{name:"Previous"})).toHaveAttribute("href","/webradios?name=Wave&tag=jazz");
  expect(playStation).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole("button",{name:"Play Wave FM"}));
  expect(playStation).toHaveBeenCalledWith(station);
});

it("lets a visitor restore a local opt-out and never queries the directory when disabled globally", async () => {
  Object.assign(page, {url: new URL("https://app.test/webradios")});
  localStorage.setItem("soundtime_webradios_guest", "false");
  let global = true;
  const fetchMock = vi.fn(async (url: string) => url.endsWith("/status")
    ? Response.json({...enabled, enabled: global, instance_enabled:global})
    : Response.json({stations:[station],next_offset:null}));
  vi.stubGlobal("fetch",fetchMock);
  const view = render(Stations);
  const toggle = await screen.findByRole("switch");
  expect(toggle).not.toBeChecked();
  expect(fetchMock.mock.calls.filter(([url])=>url.includes("/stations"))).toHaveLength(0);
  await fireEvent.click(toggle);
  await screen.findByRole("heading",{name:"Wave FM"});
  view.unmount();
  global = false;
  render(Stations);
  await waitFor(()=>expect(screen.getByRole("switch")).toBeDisabled());
  expect(screen.queryByText(station.name)).toBeNull();
});
