// Drive the real playlist UI and observe its player and HTTP traffic using headless Chrome.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { pageDocuments } from "./browser-document.mjs";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createInterface } from "node:readline";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

const [base, playlist, track, title] = process.argv.slice(2);
assert.ok(base && playlist && track && title, "Expected base URL, playlist ID, track ID and title");
const origin = new URL(base).origin;
const streamPath = "/api/tracks/" + track + "/stream";
const profile = await mkdtemp(join(tmpdir(), "soundtime-browser-"));
const chrome = process.env.CHROME_BIN || (process.platform === "darwin"
  ? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" : "google-chrome");
// Container Chromium runs as an unprivileged user, with no capabilities, mounts,
// or host sockets. Docker's seccomp profile denies Chromium's nested namespace sandbox.
const containerArgs = process.env.SOUNDTIME_CONTAINER_BROWSER === "true" ? ["--no-sandbox"] : [];
const browser = spawn(chrome, [
  ...containerArgs,
  "--headless", "--lang=en-US", "--disable-background-networking", "--no-first-run", "--mute-audio",
  "--autoplay-policy=no-user-gesture-required", "--remote-debugging-port=0",
  "--remote-debugging-address=127.0.0.1", "--user-data-dir=" + profile, "about:blank",
], { stdio: ["ignore", "ignore", "pipe"] });
let chromeLog = "";
browser.stderr.setEncoding("utf8");
browser.stderr.on("data", text => {
  const output = chromeLog + text;
  chromeLog = output.length <= 1200 ? output : output.slice(0, 800) + "\n…\n" + output.slice(-397);
});
const exited = once(browser, "exit").catch(() => {});
let launchError;
browser.on("error", error => { launchError = error; });
let socket;

async function until(label, get, timeout = 20000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    if (launchError) throw launchError;
    if (browser.exitCode !== null || browser.signalCode) throw new Error("Chrome exited during " + label + " (" + (browser.signalCode || browser.exitCode) + "): " + chromeLog);
    const value = await get();
    if (value) return value;
    await delay(100);
  }
  throw new Error("Timed out waiting for " + label);
}

// Executed in the document after a visible result, relative to navigation start.
// Report fixed endpoint names only. Query strings may contain listener searches.
function navigationTimings() {
  const navigation = performance.getEntriesByType("navigation")[0];
  if (!navigation) return null;
  const resources = performance.getEntriesByType("resource");
  const endpoints = {};
  const paths = new Set(["/api/setup/status", "/api/auth/me", "/api/auth/media-session", "/api/search"]);
  let assets = 0;
  let transferred = 0;
  let lastAssetEnd = 0;
  for (const resource of resources) {
    const url = new URL(resource.name);
    if (url.origin !== location.origin) continue;
    if (paths.has(url.pathname)) {
      endpoints[url.pathname] = {
        start_ms: resource.startTime, first_byte_ms: resource.responseStart, end_ms: resource.responseEnd,
      };
    } else if (url.pathname.startsWith("/_app/immutable/")) {
      assets++;
      transferred += resource.transferSize;
      lastAssetEnd = Math.max(lastAssetEnd, resource.responseEnd);
    }
  }
  return {
    document_first_byte_ms: navigation.responseStart, document_end_ms: navigation.responseEnd,
    dom_interactive_ms: navigation.domInteractive, immutable_assets: assets,
    immutable_transfer_bytes: transferred, last_asset_end_ms: lastAssetEnd, endpoints,
  };
}

// Install these DOM lookups in each new document. Fixture values are passed by CDP.
function installPlaylistProbe() {
  window.soundtimePlaylistProbe = {
    row(fixture) {
      return Array.from(document.querySelectorAll('[role="group"]')).find(row =>
        row.getAttribute("aria-label") === fixture.title &&
        row.querySelector("a")?.getAttribute("href") === "/tracks/" + fixture.track);
    },
    visibleSeek(scope) {
      return Array.from(document.querySelectorAll(scope + ' input[type="range"][aria-label="Playback position"]'))
        .find(el => !el.disabled && el.getClientRects().length > 0);
    },
  };
}

try {
  const port = await until("Chrome debugger", async () => {
    try {
      return (await readFile(join(profile, "DevToolsActivePort"), "utf8")).split("\n")[0];
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  });
  const tabs = await (await fetch("http://127.0.0.1:" + port + "/json/list")).json();
  const page = tabs.find(tab => tab.type === "page");
  assert.ok(page, "Chrome has no page target");
  socket = new WebSocket(page.webSocketDebuggerUrl);
  await once(socket, "open", { signal: AbortSignal.timeout(10000) });
  const pending = new Map();
  const responses = [];
  const requests = [];
  const mutations = [];
  const logins = [];
  let sequence = 0;
  const documents = pageDocuments(send);
  socket.addEventListener("message", ({ data }) => {
    const msg = JSON.parse(data);
    documents.observe(msg);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      if (msg.error) reject(Object.assign(new Error(msg.error.message), {code: msg.error.code}));
      else resolve(msg.result);
      return;
    }
    if (msg.method === "Network.requestWillBeSent") {
      const url = new URL(msg.params.request.url);
      if (url.origin === origin) requests.push(url.pathname);
    }
    if (msg.method === "Network.responseReceived") {
      const response = msg.params.response;
      const url = new URL(response.url);
      if (url.origin === origin && url.pathname === "/api/auth/login") {
        const value = Object.entries(response.headers).find(([name]) => name.toLowerCase() === "retry-after")?.[1];
        logins.push({status:response.status, retry:value});
      }
      if (url.origin === origin && url.pathname.startsWith("/api/playlists/") && response.status >= 200) {
        mutations.push({path:url.pathname,status:response.status});
      }
      if (url.origin === origin && url.pathname === streamPath) {
        responses.push({ status: response.status, type: response.mimeType });
      }
    }
  });
  async function send(method, params = {}) {
    const id = ++sequence;
    let timer;
    try {
      return await new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        timer = setTimeout(() => reject(new Error("Chrome command timed out: " + method)), 10000);
        socket.send(JSON.stringify({ id, method, params }));
      });
    } finally {
      clearTimeout(timer);
      pending.delete(id);
    }
  }
  async function evaluate(callback, {input = null, userGesture = false, awaitPromise = false} = {}) {
    const value = await documents.call({
      functionDeclaration: callback.toString(),
      arguments: [{ value: { title, track, playlist } }, { value: input }],
      returnByValue: true, userGesture, awaitPromise,
    }, !userGesture);
    if (value.exceptionDetails) throw new Error(value.exceptionDetails.text);
    return value.result.value;
  }

  await documents.enable();
  await send("Network.enable");
  await send("Emulation.setLocaleOverride", {locale:"en-US"});
  await send("Page.addScriptToEvaluateOnNewDocument", {source: "(" + installPlaylistProbe.toString() + ")()"});
  const playerSelector = '[data-testid="audio-player"]';

  async function viewport(mobile) {
    const width = mobile ? 390 : 1440;
    const height = mobile ? 844 : 900;
    await send("Emulation.setDeviceMetricsOverride", {width, height, mobile, deviceScaleFactor:1});
    await send("Emulation.setTouchEmulationEnabled", {enabled:mobile});
    await until("requested viewport", () => evaluate((_, size) =>
      innerWidth === size.width && innerHeight === size.height, {input: {width, height}}));
  }
  async function screenshot() {
    await evaluate(() => document.fonts.ready.then(() =>
      new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))), {awaitPromise: true});
    const {data} = await send("Page.captureScreenshot", {format:"png", fromSurface:true, captureBeyondViewport:false});
    return data;
  }
  let activeListener;
  async function asListener(listener) {
    if(activeListener===listener.username) return;
    await documents.navigate("about:blank");
    await until("clean navigation",()=>evaluate(() => location.href === "about:blank"));
    await send("Storage.clearDataForOrigin",{origin,storageTypes:"cookies,local_storage"});
    const destination="/playlists/"+listener.playlist;
    await documents.navigate(origin+"/login?next="+encodeURIComponent(destination));
    await until("login form",()=>evaluate(() => Boolean(document.querySelector("form input[name=password]"))));
    const submit = () => evaluate((_, credentials) => {
      const form=document.querySelector("form input[name=password]").form;
      for(const [name,value] of Object.entries(credentials)){
        const field=form.elements.namedItem(name);
        field.value=value;
        field.dispatchEvent(new Event("input",{bubbles:true}));
      }
      form.requestSubmit();
    }, {userGesture: true, input: {username:listener.username,password:listener.password}});
    const deadline = Date.now() + 60000;
    for (;;) {
      const checkpoint = logins.length;
      await submit();
      await until("login HTTP response",()=>logins.length>checkpoint);
      const {status,retry} = logins.at(-1);
      if (status !== 429) {
        assert.equal(status,200,"Fixture account could not log in through the UI");
        break;
      }
      const milliseconds = /^\d+$/.test(retry || "") ? Number(retry)*1000 : Date.parse(retry)-Date.now();
      const wait = Math.max(1000, Number.isFinite(milliseconds) ? milliseconds : 6000) + 250;
      assert.ok(Date.now()+wait < deadline, "Login cooldown exceeded preparation budget");
      await delay(wait);
    }
    await until("signed-in listener's playlist",() => evaluate((_, path) =>
      location.pathname === path && Boolean(document.querySelector("button.play-cell")) &&
      !!localStorage.getItem("soundtime_access_token"), {input: destination}));
    activeListener=listener.username;
  }
  async function click(selector, label, {text = null, inTrack = false} = {}) {
    try {
      await until(label, () => evaluate((fixture, target) => {
        const scope = target.inTrack ? window.soundtimePlaylistProbe.row(fixture) : document;
        if (!scope) return false;
        const element = target.text === null ? scope.querySelector(target.selector) :
          Array.from(scope.querySelectorAll(target.selector)).find(el => el.textContent.trim() === target.text);
        if (!element) return false;
        element.click();
        return true;
      }, {userGesture: true, input: {selector, text, inTrack}}));
    } catch(error) {
      const state=await evaluate(() => ({
        path:location.pathname, menu:[...document.querySelectorAll('[role=menuitem]')].map(el=>el.textContent.trim()),
        dialog:document.querySelector('[role=dialog] h2')?.textContent,
      }));
      throw new Error(error.message+" "+JSON.stringify(state));
    }
  }
  function noMedia(start) {
    const paths=requests.slice(start);
    assert.ok(!paths.some(path=>path.endsWith("/stream") || path.endsWith("/stream/best")),
      "Playlist or metadata operation opened an audio stream");
    assert.ok(!paths.includes("/api/p2p/search"),"Normal search queried the wider network");
  }
  async function checkRowCredit(credit) {
    const labels = await evaluate(fixture => {
      const item = window.soundtimePlaylistProbe.row(fixture);
      return {
        artist_name: item.querySelector('a[href^="/tracks/"] ~ span')?.innerText.trim(),
        album_title: item.querySelector('a[href^="/albums/"]')?.innerText.trim(),
      };
    });
    assert.deepEqual(labels, credit, "The indexed recording must retain its artist and album in this view");
  }
  async function addToPlaylists(listeners, capture, credit) {
    const start=requests.length;
    const added=[];
    const screenshots = {};
    for (const listener of listeners) {
      const checkpoint=mutations.length;
      await asListener(listener);
      await documents.navigate(origin + "/search?q=" + encodeURIComponent(title));
      if (capture && !added.length) {
        await viewport(false);
        await until("desktop search result", () => evaluate(fixture =>
          location.pathname === "/search" && Boolean(window.soundtimePlaylistProbe.row(fixture)?.getClientRects().length)));
        if (credit) await checkRowCredit(credit);
        screenshots["search-desktop"] = await screenshot();
      }
      await click("[aria-haspopup=menu]", "indexed remote track", {inTrack: true});
      await click("[role=menuitem]", "playlist action", {text: "Add to playlist"});
      await click("[role=dialog] button", "listener's playlist", {text: listener.name});
      const path="/api/playlists/" + listener.playlist + "/tracks";
      await until("new playlist membership",()=>mutations.slice(checkpoint).some(m=>m.path===path && m.status===201));
      added.push(listener.playlist);
    }
    noMedia(start);
    return {added, stream_requests:0, extended_search_requests:0, ...(capture ? {screenshots} : {})};
  }
  async function openPlaylist(listener) {
    await asListener(listener);
    await documents.navigate(origin + "/playlists/" + listener.playlist);
    await until("listener's playlist",()=>evaluate((fixture, path) =>
      location.pathname === path && Boolean(window.soundtimePlaylistProbe.row(fixture)), {input: "/playlists/" + listener.playlist}));
  }
  async function withdrawals(listeners) {
    const start=requests.length;
    const disabled=[];
    for (const listener of listeners) {
      await openPlaylist(listener);
      const state=await evaluate(fixture => {
        const row = window.soundtimePlaylistProbe.row(fixture);
        return {
          availability:row.dataset.availability, disabled:row.querySelector('.play-cell').disabled,
          position:Number(row.querySelector('.track-number').textContent.trim())-1,
          text:row.innerText.includes('Removed by the publisher'),
        };
      });
      assert.deepEqual(state,{availability:"withdrawn",disabled:true,position:1,text:true});
      disabled.push(listener.playlist);
    }
    const owner=listeners[0];
    await openPlaylist(owner);
    const checkpoint=mutations.length;
    await click("[aria-haspopup=menu]", "track options", {inTrack: true});
    await click("[role=menuitem]", "remove own entry", {text: "Remove from this playlist"});
    const path="/api/playlists/"+owner.playlist+"/tracks/"+track;
    await until("playlist removal",()=>mutations.slice(checkpoint).some(m=>m.path===path && m.status===204));
    await until("withdrawn entry removed from display",()=>evaluate(fixture =>
      !window.soundtimePlaylistProbe.row(fixture) && document.querySelectorAll("button.play-cell").length === 2));
    await openPlaylist(listeners[1]);
    assert.equal(await evaluate(fixture => window.soundtimePlaylistProbe.row(fixture)?.querySelector("button.play-cell").disabled), true);
    noMedia(start);
    return {disabled,removed_from:owner.playlist,other_playlist_unchanged:true,stream_requests:0};
  }
  async function firstSound(listener) {
    await asListener(listener);
    await send("Network.setCacheDisabled", {cacheDisabled: true});
    await openPlaylist(listener);
    const checkpoint = responses.length;
    // The player uses a detached HTMLAudioElement. Observe its first actual 'playing'
    // event on the same clock as the click, then require decoded time to advance.
    await evaluate(fixture => {
      const target = window.soundtimePlaylistProbe.row(fixture).querySelector("button.play-cell");
      const original = HTMLMediaElement.prototype.play;
      const sample = window.soundtimeTestPlayback = {start: performance.now()};
      HTMLMediaElement.prototype.play = function(...args) {
        HTMLMediaElement.prototype.play = original;
        sample.audio = this;
        this.addEventListener("playing", () => {
          sample.first = performance.now() - sample.start;
        }, {once:true});
        return original.apply(this, args);
      };
      target.click();
    }, {userGesture: true});
    const observation = await until("first decoded audio", () => evaluate(() => {
      const s = window.soundtimeTestPlayback;
      if (s?.first > 0 && s.audio.currentTime > 0) {
        return {first_sound_ms:s.first, decoded_seconds:s.audio.currentTime};
      }
    }));
    assert.ok(responses.slice(checkpoint).some(r => r.status === 206 || r.status === 200),
      "No new media HTTP response during the sample");
    await documents.navigate("about:blank");
    await until("sample player closed", () => evaluate(() => location.href === "about:blank"));
    return observation;
  }
  async function seekThroughPlayer(scope) {
    await until("visible enabled seek control", () => evaluate((_, scope) =>
      Boolean(window.soundtimePlaylistProbe.visibleSeek(scope)), {input: scope}));
    const before = await evaluate(() => window.soundtimeTestAudio.currentTime);
    const target = Math.ceil(before) + 6;
    await evaluate((_, {scope, target}) => {
      const audio = window.soundtimeTestAudio;
      window.soundtimeTestSeek = null;
      audio.addEventListener("seeked", () => { window.soundtimeTestSeek = audio.currentTime; }, {once:true});
      const control = window.soundtimePlaylistProbe.visibleSeek(scope);
      control.value = target;
      control.dispatchEvent(new Event("input", {bubbles:true}));
    }, {userGesture: true, input: {scope, target}});
    await until("media seeked to requested position", () => evaluate((_, target) =>
      window.soundtimeTestSeek !== null && Math.abs(window.soundtimeTestSeek - target) < 1, {input: target}));
    const after = await until("decoded progress after seek", () => evaluate((_, target) =>
      window.soundtimeTestAudio.currentTime >= target + 0.5 && window.soundtimeTestAudio.currentTime, {input: target}));
    return {before_seek_seconds:before, seek_to_seconds:target, after_seek_seconds:after};
  }
  async function openExpandedPlayer() {
    await click(playerSelector + " button.open-player", "expand the current track");
    await until("expanded player for the fixture track", () => evaluate(fixture =>
      Array.from(document.querySelectorAll('[role=dialog] h2')).some(h => h.textContent.trim() === fixture.title)
        && Boolean(window.soundtimePlaylistProbe.visibleSeek("[role=dialog]"))));
  }
  async function playback(capture = false, credit) {
    await documents.navigate(origin + "/playlists/" + encodeURIComponent(playlist));
    await until("the remote track in its playlist", () => evaluate(fixture =>
      location.pathname === "/playlists/" + fixture.playlist &&
      Boolean(window.soundtimePlaylistProbe.row(fixture)?.querySelector("button.play-cell"))));
    if (capture) await viewport(false);
    if (credit) await checkRowCredit(credit);
    const checkpoint = responses.length;
    const started = performance.now();
    // Observe the detached HTMLAudioElement actually used by SoundTime.
    await evaluate(fixture => {
      const original = HTMLMediaElement.prototype.play;
      HTMLMediaElement.prototype.play = function(...args) {
        HTMLMediaElement.prototype.play = original;
        window.soundtimeTestAudio = this;
        return original.apply(this, args);
      };
      window.soundtimePlaylistProbe.row(fixture).querySelector("button.play-cell").click();
    }, {userGesture: true});
    // Keep the first-progress measurement tied to the UI update as well as the
    // media clock. The collapsed mobile player renders its progress as a CSS bar.
    const time = () => evaluate((_, playerSelector) => {
      const audio = window.soundtimeTestAudio;
      const bar = document.querySelector(playerSelector);
      if (!audio || !bar || !(audio.duration > 0)) return 0;
      const slider = window.soundtimePlaylistProbe.visibleSeek(playerSelector);
      const shown = slider ? slider.valueAsNumber : parseFloat(bar.style.getPropertyValue("--progress")) * audio.duration / 100;
      return Math.min(audio.currentTime, shown);
    }, {input: playerSelector});
    await until("first decoded and displayed media progress", async () => (await time()) > 0);
    const firstProgressMs = Math.round(performance.now() - started);
    await until("one second of playback", async () => (await time()) >= 1);
    const hasCompactSeek = () => evaluate((_, scope) => Boolean(window.soundtimePlaylistProbe.visibleSeek(scope)),
      {input: playerSelector});
    const compact = await hasCompactSeek();
    if (!compact) await openExpandedPlayer();
    const seek = await seekThroughPlayer(compact ? playerSelector : "[role=dialog]");
    const currentResponses = responses.slice(checkpoint);
    assert.ok(currentResponses.some(r => r.status === 206 || r.status === 200), "Missing successful audio HTTP response");
    const result = { first_progress_ms:firstProgressMs, ...seek, stream_responses:currentResponses };
    if (capture) {
      assert.ok(compact, "Desktop playlist has no visible player timeline");
      if (credit) {
        const artist = await evaluate((_, scope) => document.querySelector(scope + " .artist")?.innerText.trim(),
          {input: playerSelector});
        assert.equal(artist, credit.artist_name, "The mini-player lost the saved artist credit");
      }
      const desktop = await screenshot();
      await viewport(true);
      assert.equal(await hasCompactSeek(), false,
        "Mobile mini-player should expose seeking via the expanded player");
      await openExpandedPlayer();
      if (credit) {
        const details = await evaluate(() => ({
          artist_name: document.querySelector('[role=dialog] a[href^="/artists/"]')?.innerText.trim(),
          album_title: document.querySelector('[role=dialog] a[href^="/albums/"]')?.innerText.trim(),
        }));
        assert.deepEqual(details, credit, "The expanded player lost the saved credit");
      }
      result.mobile_seek = await seekThroughPlayer("[role=dialog]");
      result.screenshots = {"playlist-desktop":desktop, "player-mobile":await screenshot()};
    }
    return result;
  }

  if (process.argv.includes("--session")) {
    const commands=createInterface({input:process.stdin,crlfDelay:Infinity});
    for await(const line of commands) {
      const {phase,listeners,capture=false,credit}=JSON.parse(line);
      let result;
      if (phase==="add") result=await addToPlaylists(listeners, capture, credit);
      else if (phase==="authenticate") {
        await asListener(listeners[0]);
        result={authenticated:true, attempts:logins.length};
      }
      else if (phase==="first_sound") result=await firstSound(listeners[0]);
      else if (phase==="measure") {
        await asListener(listeners[0]);
        const began=performance.now();
        await documents.navigate(origin+"/search?q="+encodeURIComponent(title));
        await until("search result visible",()=>evaluate(fixture =>
          location.pathname === "/search" && new URL(location.href).searchParams.get("q") === fixture.title &&
          Boolean(window.soundtimePlaylistProbe.row(fixture)?.getBoundingClientRect().height)));
        const visible=performance.now()-began;
        const navigation = await evaluate(navigationTimings);
        result={search_to_screen_ms:visible,navigation_timing:navigation,...await playback()};
        await documents.navigate("about:blank");
      }
      else if (phase==="play") {
        await asListener(listeners[0]);
        result=await playback(capture, credit);
        await documents.navigate("about:blank");
        if (capture) await viewport(false);
      } else if (phase==="withdraw") result=await withdrawals(listeners);
      else throw new Error("Unknown browser phase");
      console.log(JSON.stringify(result));
    }
  } else {
    console.log(JSON.stringify(await playback()));
  }
} finally {
  socket?.close();
  browser.kill("SIGTERM");
  await Promise.race([exited, delay(1500, undefined, { ref: false })]);
  if (browser.exitCode === null && !browser.signalCode && browser.pid) browser.kill("SIGKILL");
  await exited;
  await rm(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}
