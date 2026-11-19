// Drive the real playlist UI and observe its player and HTTP traffic using headless Chrome.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
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
const browser = spawn(chrome, [
  "--headless", "--disable-background-networking", "--no-first-run", "--mute-audio",
  "--autoplay-policy=no-user-gesture-required", "--remote-debugging-port=0",
  "--remote-debugging-address=127.0.0.1", "--user-data-dir=" + profile, "about:blank",
], { stdio: "ignore" });
const exited = once(browser, "exit").catch(() => {});
let launchError;
browser.on("error", error => { launchError = error; });
let socket;

async function until(label, get, timeout = 20000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    if (launchError) throw launchError;
    if (browser.exitCode !== null || browser.signalCode) throw new Error("Chrome exited during " + label);
    const value = await get();
    if (value) return value;
    await delay(100);
  }
  throw new Error("Timed out waiting for " + label);
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
  let sequence = 0;
  socket.addEventListener("message", ({ data }) => {
    const msg = JSON.parse(data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      if (msg.error) reject(new Error(msg.error.message));
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
  async function evaluate(expression, userGesture = false) {
    const value = await send("Runtime.evaluate", { expression, returnByValue: true, userGesture });
    if (value.exceptionDetails) throw new Error(value.exceptionDetails.text);
    return value.result.value;
  }

  await send("Network.enable");
  const row = `Array.from(document.querySelectorAll('[role="group"]'))
    .find(row => row.getAttribute('aria-label') === ${JSON.stringify(title)}
      && row.querySelector('a')?.getAttribute('href') === ${JSON.stringify("/tracks/" + track)})`;
  const button = row + "?.querySelector('button.play-cell')";
  let injection;
  async function asListener(listener) {
    await send("Page.navigate", {url:"about:blank"});
    await until("clean navigation", () => evaluate("location.href === 'about:blank'"));
    if (injection) await send("Page.removeScriptToEvaluateOnNewDocument",{identifier:injection});
    const result=await send("Page.addScriptToEvaluateOnNewDocument",{source:
      "if(location.origin===" + JSON.stringify(origin) + "){localStorage.setItem('soundtime_lang','en');" +
      "localStorage.setItem('soundtime_access_token'," + JSON.stringify(listener.token) + ");}"});
    injection=result.identifier;
  }
  async function click(expression,label) {
    await until(label, () => evaluate(`Boolean(${expression})`));
    await evaluate(`(${expression}).click()`,true);
  }
  function byText(selector,label) {
    return `Array.from(document.querySelectorAll(${JSON.stringify(selector)})).find(el=>el.textContent.trim()===${JSON.stringify(label)})`;
  }
  function noMedia(start) {
    const paths=requests.slice(start);
    assert.ok(!paths.some(path=>path.endsWith("/stream") || path.endsWith("/stream/best")),
      "Playlist or metadata operation opened an audio stream");
    assert.ok(!paths.includes("/api/p2p/search"),"Normal search queried the wider network");
  }
  async function addToPlaylists(listeners) {
    const start=requests.length;
    const added=[];
    for (const listener of listeners) {
      const checkpoint=mutations.length;
      await asListener(listener);
      await send("Page.navigate",{url:origin + "/search?q=" + encodeURIComponent(title)});
      await click(row + "?.querySelector('[aria-haspopup=menu]')","indexed remote track");
      await click(byText('[role=menuitem]',"Add to playlist"),"playlist action");
      await click(byText('[role=dialog] button',listener.name),"listener's playlist");
      const path="/api/playlists/" + listener.playlist + "/tracks";
      await until("new playlist membership",()=>mutations.slice(checkpoint).some(m=>m.path===path && m.status===201));
      added.push(listener.playlist);
    }
    noMedia(start);
    return {added, stream_requests:0, extended_search_requests:0};
  }
  async function openPlaylist(listener) {
    await asListener(listener);
    await send("Page.navigate",{url:origin + "/playlists/" + listener.playlist});
    await until("listener's playlist",()=>evaluate("location.pathname===" + JSON.stringify("/playlists/" + listener.playlist)
      + " && Boolean(" + row + ")"));
  }
  async function withdrawals(listeners) {
    const start=requests.length;
    const disabled=[];
    for (const listener of listeners) {
      await openPlaylist(listener);
      const state=await evaluate(`(()=>{const r=${row}; return {
        availability:r.dataset.availability,disabled:r.querySelector('.play-cell').disabled,
        position:Number(r.querySelector('.track-number').textContent.trim())-1,
        text:r.innerText.includes('Removed by the publisher')}})()`);
      assert.deepEqual(state,{availability:"withdrawn",disabled:true,position:1,text:true});
      disabled.push(listener.playlist);
    }
    const owner=listeners[0];
    await openPlaylist(owner);
    const checkpoint=mutations.length;
    await click(row+"?.querySelector('[aria-haspopup=menu]')","track options");
    await click(byText('[role=menuitem]',"Remove from this playlist"),"remove own entry");
    const path="/api/playlists/"+owner.playlist+"/tracks/"+track;
    await until("playlist removal",()=>mutations.slice(checkpoint).some(m=>m.path===path && m.status===204));
    await until("withdrawn entry removed from display",()=>evaluate("!Boolean("+row+") && document.querySelectorAll('button.play-cell').length===2"));
    await openPlaylist(listeners[1]);
    assert.equal(await evaluate(button+".disabled"),true);
    noMedia(start);
    return {disabled,removed_from:owner.playlist,other_playlist_unchanged:true,stream_requests:0};
  }
  async function playback() {
    await send("Page.navigate", { url: origin + "/playlists/" + encodeURIComponent(playlist) });
    await until("the remote track in its playlist", () => evaluate("Boolean(" + button + ")"));
    const started = performance.now();
    await evaluate(button + ".click()", true);
    // This slider is updated by timeupdate on the player's own HTMLAudioElement.
    const slider = 'document.querySelector(\'[data-testid="audio-player"] input[step="any"]\')';
    const time = () => evaluate("Number(" + slider + "?.value || 0)");
    await until("first media progress", async () => (await time()) > 0);
    const firstProgressMs = Math.round(performance.now() - started);
    const beforeSeek = await until("one second of playback", async () => {
      const seconds = await time();
      return seconds >= 1 && seconds;
    });
    const target = Math.ceil(beforeSeek) + 6;
    await evaluate("(() => {const s = " + slider + "; s.value = " + target +
      "; s.dispatchEvent(new Event('input', {bubbles:true}));})()", true);
    const afterSeek = await until("media progress after seek", async () => {
      const seconds = await time();
      return seconds >= target + 0.5 && seconds;
    });
    assert.ok(responses.some(r => r.status === 206 || r.status === 200), "Missing successful audio HTTP response");
    return { first_progress_ms: firstProgressMs,
      before_seek_seconds: beforeSeek, seek_to_seconds: target, after_seek_seconds: afterSeek,
      stream_responses: [...responses] };
  }

  if (process.argv.includes("--session")) {
    const commands=createInterface({input:process.stdin,crlfDelay:Infinity});
    for await(const line of commands) {
      const {phase,listeners}=JSON.parse(line);
      let result;
      if (phase==="add") result=await addToPlaylists(listeners);
      else if (phase==="play") {
        await asListener(listeners[0]);
        result=await playback();
        await send("Page.navigate",{url:"about:blank"});
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
