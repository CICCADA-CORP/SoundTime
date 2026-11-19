import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { page } from "$app/state";
import { setLocale } from "$lib/i18n/index.svelte";
import Stations from "./+page.svelte";

const { playStation, playPodcast } = vi.hoisted(() => ({ playStation: vi.fn(), playPodcast: vi.fn() }));
vi.mock("$lib/stores/player.svelte", () => ({ getPlayerStore: () => ({ playStation, playPodcast }) }));

const station = { id: "77000000-0000-4000-8000-000000000001", name: "Wave FM", icon_url: "https://wave.test/icon.png",
  description: "Independent jazz station", tags: ["jazz", "swing"], country: "FR", language: "French", codec: "AAC", bitrate: 128, hls: false };
const enabled = { instance_enabled: true, user_enabled: true, enabled: true };

beforeEach(() => {
  window.dispatchEvent(new Event("soundtime:logout"));
  vi.clearAllMocks();
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
    : url.includes("/podcasts") ? Response.json({shows:[]}) : Response.json({stations:[station],next_offset:null}));
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

const show = {id:"123456",title:"Studio Notes",author:"SoundTime Studio",genre:"Music",artwork_url:null};
const episode = {id:"episode-a",title:"Building a radio show",description:"Inside the studio.",published_at:"2026-09-05T18:00:00Z",audio_url:"https://media.example.org/episode.wav",duration:3600};

it("loads both directories on Discover and links to the full show without fetching its episodes or audio", async () => {
  Object.assign(page, {url: new URL("https://app.test/webradios")});
  const fetchMock = vi.fn(async (url:string) => {
    if (url.endsWith("/status")) return Response.json(enabled);
    if (url.includes("/podcasts?")) return Response.json({shows:[show]});
    return Response.json({stations:[station],next_offset:null});
  });
  vi.stubGlobal("fetch",fetchMock);
  render(Stations);
  const item = await screen.findByRole("link",{name:/Studio Notes/});
  expect(item).toHaveAttribute("href","/webradios?view=podcasts&country=US&show=123456");
  expect(await screen.findByRole("heading",{name:"Wave FM"})).toBeTruthy();
  expect(fetchMock.mock.calls.some(([url])=>/episodes|episode.wav/.test(url))).toBe(false);
  expect(playPodcast).not.toHaveBeenCalled();
});

it("searches a selected Apple storefront and keeps search separate from episode playback", async () => {
  Object.assign(page, {url: new URL("https://app.test/webradios?view=podcasts&q=Studio&country=ca")});
  const fetchMock = vi.fn(async (url:string) => {
    if (url.endsWith("/status")) return Response.json(enabled);
    expect(url).toContain("/webradios/podcasts?q=Studio&country=CA");
    return Response.json({shows:[show]});
  });
  vi.stubGlobal("fetch",fetchMock);
  render(Stations);
  const link = await screen.findByRole("link",{name:/Studio Notes/});
  expect(link.getAttribute("href")).toBe("/webradios?view=podcasts&country=CA&q=Studio&show=123456");
  expect(screen.getByRole("searchbox",{name:"Show or creator"})).toHaveValue("Studio");
  expect(screen.getByRole("combobox",{name:"Store country"})).toHaveValue("CA");
  expect(playPodcast).not.toHaveBeenCalled();
});

it("opens a show by URL, reads its public episodes and starts audio only on the selected episode", async () => {
  Object.assign(page, {url:new URL("https://app.test/webradios?view=podcasts&show=123456&country=FR")});
  vi.stubGlobal("fetch",vi.fn(async (url:string) => {
    if (url.endsWith("/status")) return Response.json(enabled);
    expect(url).toContain("/podcasts/123456/episodes?country=FR&offset=0");
    return Response.json({show,episodes:[episode],next_offset:50,truncated:false});
  }));
  render(Stations);
  const play = await screen.findByRole("button",{name:"Play Building a radio show"});
  expect(screen.getByRole("link",{name:"Apple Podcasts"})).toHaveAttribute("href","https://podcasts.apple.com/fr/podcast/id123456");
  expect(screen.getByRole("link",{name:"Next"})).toHaveAttribute("href","/webradios?view=podcasts&country=FR&show=123456&offset=50");
  expect(playPodcast).not.toHaveBeenCalled();
  await fireEvent.click(play);
  expect(playPodcast).toHaveBeenCalledWith(show,episode,"FR");
});
