// Node 22+, Chrome, and a disposable instance. Pass URL, artifact directory and
// a temporary admin's {"username":"…","password":"…"} on stdin. Only directory,
// playlist and audio fixtures are intercepted. Permissions use the real backend.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

const [url, output] = process.argv.slice(2);
assert.ok(url && output, "Expected instance URL and artifact directory");
const origin = new URL(url).origin;
const piped = output === "--pipe";
const artifacts = piped ? undefined : resolve(output);
if (artifacts) await mkdir(artifacts, {recursive:true});
const screenshots = {};
const report = {checks:[], directory:[], clicks:0, media:[], blocked_external_requests:0, screenshots:[]};
const ids = {
  station:"a8afdd22-4000-4000-8400-000000000001", second:"a8afdd22-4000-4000-8400-000000000002",
  playlist:"4c34a12d-4000-4000-8400-000000000001", tracks:["8dcdb546-4000-4000-8400-000000000001","8dcdb546-4000-4000-8400-000000000002"],
};
const liveUrl = "https://soundtime-fixture.invalid/live.wav";
const podcastUrl = "https://soundtime-fixture.invalid/episode.wav";
const show = {id:"123456",title:"After Hours",author:"Studio West",artwork_url:null,genre:"Music"};
const episode = {id:"episode-a",title:"Inside the sound booth",description:"How the desk works.",published_at:"2026-10-01T14:00:00Z",audio_url:podcastUrl,duration:24};
const station = {id:ids.station,name:"Night Wave",icon_url:null,description:"Night-time jazz from the fixture studio.",
  tags:["jazz","live"],country:"France",language:"French",codec:"PCM",bitrate:128,hls:false};
const tracks = ids.tracks.map((id,i)=>({id,title:"Queue witness "+(i+1),artist_name:"Fixture Ensemble",artist_id:ids.playlist,
    album_id:null,duration_secs:24,file_path:"fixture.wav",file_size:384044,format:"wav",play_count:0,availability:"playable",created_at:"2026-01-01T00:00:00Z"}));

function pcmWave() {
  const rate = 8000, samples = rate*24, wav = Buffer.alloc(44 + samples*2);
  wav.write("RIFF"); wav.writeUInt32LE(wav.length-8,4); wav.write("WAVEfmt ",8);
  wav.writeUInt32LE(16,16); wav.writeUInt16LE(1,20); wav.writeUInt16LE(1,22);
  wav.writeUInt32LE(rate,24); wav.writeUInt32LE(rate*2,28); wav.writeUInt16LE(2,32);
  wav.writeUInt16LE(16,34); wav.write("data",36); wav.writeUInt32LE(samples*2,40);
  for (let i=0;i<samples;i++) wav.writeInt16LE(Math.round(1600*Math.sin(2*Math.PI*330*i/rate)),44+i*2);
  return wav;
}
const wav = pcmWave();
const q = JSON.stringify;
const deadline = (ms=12000) => AbortSignal.timeout(ms);
let token;
async function api(path, method="GET", data) {
  const response = await fetch(origin+"/api"+path,{method,signal:deadline(),redirect:"error",
    headers:{"Content-Type":"application/json",...(token ? {Authorization:"Bearer "+token} : {})},
    ...(data ? {body:JSON.stringify(data)} : {})});
  assert.ok(response.ok, method+" "+path+" HTTP "+response.status);
  return response.status===204 ? undefined : response.json();
}
async function login() {
  let credentials = {username:process.env.WEBRADIO_ADMIN_USER,password:process.env.WEBRADIO_ADMIN_PASSWORD};
  if (!credentials.password) {
    let buffer="";
    for await(const chunk of process.stdin) buffer+=chunk;
    credentials=JSON.parse(buffer);
  }
  if (credentials.access_token && credentials.refresh_token) {
    token = credentials.access_token;
    assert.equal((await api("/auth/me")).role,"admin","Expected disposable admin token");
    return credentials;
  }
  const session=await api("/auth/login","POST",credentials);
  assert.equal(session.user.role,"admin","Use a disposable admin fixture");
  return session.tokens;
}

// A private, null-delimited debugging pipe avoids a listening debugger port.
function openChrome(profile) {
  const exe=process.env.CHROME_BIN || (process.platform==="darwin" ? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome":"google-chrome");
  const child=spawn(exe,[...(process.env.SOUNDTIME_CONTAINER_BROWSER==="true"?["--no-sandbox"]:[]),
    "--remote-debugging-pipe","--headless","--mute-audio","--lang=en-US","--disable-background-networking",
    "--autoplay-policy=no-user-gesture-required","--no-first-run","--user-data-dir="+profile,"about:blank"],
    {stdio:["ignore","ignore","pipe","pipe","pipe"]});
  let id=0, buffer="", stderr="", session;
  const pending=new Map();
  let handle = async () => {};
  let failure;
  child.stderr.setEncoding("utf8");
  child.stderr.on("data",part=>{stderr=(stderr+part).slice(-1500);});
  const ended=once(child,"exit").catch(()=>{});
  function fail(error) {
    failure ??= error;
    for(const call of pending.values()) call.reject(error);
  }
  child.once("error",fail);
  child.once("exit",()=>fail(new Error("Chrome exited. "+stderr)));
  child.stdio[3].on("error",fail);
  child.stdio[4].setEncoding("utf8");
  child.stdio[4].on("data",part=>{
    buffer+=part;
    let at;
    while((at=buffer.indexOf("\0"))!==-1) {
      const message=JSON.parse(buffer.slice(0,at)); buffer=buffer.slice(at+1);
      if(message.id) {
        const call=pending.get(message.id);
        if(message.error) call?.reject(new Error(message.error.message));
        else call?.resolve(message.result);
      } else if(message.method==="Fetch.requestPaused") {
        handle(message.params).catch(error=>{
          // Client-side navigation can cancel an in-flight intercepted request.
          if (!/Invalid InterceptionId|Invalid interception id/i.test(error.message)) fail(error);
        });
      }
    }
  });
  async function command(method,params={},sid=session) {
    if(failure) throw failure;
    const seq=++id;
    let timer;
    try {
      return await new Promise((resolve,reject)=>{
        pending.set(seq,{resolve,reject});
        timer=setTimeout(()=>reject(new Error("Chrome timed out on "+method)),10000);
        child.stdio[3].write(JSON.stringify({id:seq,method,params,sessionId:sid})+"\0");
      });
    } finally { clearTimeout(timer); pending.delete(seq); }
  }
  async function attach() {
    const {targetInfos}=await command("Target.getTargets");
    const page=targetInfos.find(t=>t.type==="page");
    assert.ok(page,"Missing page target");
    session=(await command("Target.attachToTarget",{targetId:page.targetId,flatten:true})).sessionId;
    await command("Page.enable");
    await command("Network.enable");
    await command("Network.setCacheDisabled",{cacheDisabled:true});
  }
  async function evaluate(expression,userGesture=false) {
    const result=await command("Runtime.evaluate",{expression,returnByValue:true,awaitPromise:true,userGesture});
    if(result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    return result.result.value;
  }
  async function close() {
    if(child.exitCode===null && !child.signalCode) child.kill();
    await Promise.race([ended,delay(1500)]);
    if(child.exitCode===null && !child.signalCode) {child.kill("SIGKILL");await ended;}
  }
  return {attach,command,evaluate,close,intercept:fn=>{handle=fn;}};
}

const sleep=ms=>delay(ms);
async function run(chrome,tokens) {
  const {command,evaluate}=chrome;
  async function wait(label,fn) {
    const end=Date.now()+12000;
    while(Date.now()<end) {
      const value=await fn();
      if(value) return value;
      await sleep(80);
    }
    throw new Error("Timed out waiting for "+label);
  }
  const visible=selector=>`Array.from(document.querySelectorAll(${q(selector)})).find(e=>e.getClientRects().length)`;
  const dom=(label,expression)=>wait(label,()=>evaluate(expression));
  async function click(expression,label) {
    await dom(label,"Boolean("+expression+")");
    await evaluate("("+expression+").click()",true);
  }
  const clickCss=sel=>click(visible(sel),sel);
  async function page(path) {
    const dest=origin+path;
    await command("Page.navigate",{url:dest});
    await dom(path,"location.href==="+q(dest)+" && !!document.querySelector('#main-content')");
  }
  async function viewport(mobile) {
    const width=mobile ? 390 : 1366, height=mobile ? 844 : 900;
    await command("Emulation.setDeviceMetricsOverride",{width,height,mobile,deviceScaleFactor:1});
    await command("Emulation.setTouchEmulationEnabled",{enabled:mobile});
    await dom("viewport",`innerWidth===${width}`);
  }
  async function capture(name) {
    await evaluate("document.fonts.ready.then(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))))");
    const {data}=await command("Page.captureScreenshot",{format:"png",captureBeyondViewport:false});
    const file="webradios-"+name+".png";
    if (piped) screenshots[file] = data;
    else await writeFile(join(artifacts,file), Buffer.from(data,"base64"));
    report.screenshots.push(file);
  }
  async function intercept({requestId,request}) {
    const u=new URL(request.url);
    const fulfill=(code,mime,body,headers={})=>command("Fetch.fulfillRequest",{requestId,responseCode:code,
      responseHeaders:Object.entries({"Content-Type":mime,"Cache-Control":"no-store",...headers}).map(([name,value])=>({name,value})),
      body:Buffer.from(body).toString("base64")});
    const json=v=>fulfill(200,"application/json",JSON.stringify(v));
    const track=ids.tracks.find(id=>u.origin===origin && u.pathname==="/api/tracks/"+id+"/stream");
    if(request.url===liveUrl || request.url===podcastUrl || track) {
      const range=Object.entries(request.headers).find(([k])=>k.toLowerCase()==="range")?.[1];
      const match=range && /^bytes=(\d+)-(\d*)$/.exec(range);
      const start=match?Number(match[1]):0, end=match?.[2]?Math.min(wav.length-1,Number(match[2])):wav.length-1;
      if(start>end) return fulfill(416,"audio/wav","",{"Content-Range":"bytes */"+wav.length});
      const payload=wav.subarray(start,end+1);
      report.media.push({source:track || (request.url===podcastUrl ? "episode" : "live"),bytes:payload.length});
      return fulfill(match?206:200,"audio/wav",payload,{"Content-Length":String(payload.length),"Accept-Ranges":"bytes",
        "Access-Control-Allow-Origin":"*",...(match?{"Content-Range":`bytes ${start}-${end}/${wav.length}`}:{})});
    }
    if(u.origin!==origin) {
      report.blocked_external_requests++;
      return command("Fetch.failRequest",{requestId,errorReason:"BlockedByClient"});
    }
    if(u.pathname==="/api/webradios/podcasts") {
      return json({shows:[show],next_offset:null});
    }
    if(u.pathname==="/api/webradios/podcasts/"+show.id+"/episodes") {
      return json({show,episodes:[episode],next_offset:null,truncated:false});
    }
    if(u.pathname==="/api/webradios/stations" && request.method==="GET") {
      const name=u.searchParams.get("name")||"", tag=u.searchParams.get("tag")||"", offset=Number(u.searchParams.get("offset")||0);
      report.directory.push({name,tag,offset});
      assert.equal(u.searchParams.get("limit"),"30");
      const matches=station.name.toLowerCase().includes(name.toLowerCase()) && (!tag || station.tags.includes(tag));
      return json({stations:matches ? [offset===30 ? {...station,id:ids.second,name:"Night Wave Two"}:station] : [],
        next_offset:matches && !offset ? 30 : null});
    }
    if(u.pathname===`/api/webradios/stations/${ids.station}/play` && request.method==="POST") {
      report.clicks++;
      return json({url:liveUrl,hls:false,relay_url:null});
    }
    // No unrecognised provider/media lookup may escape the fixture.
    if(u.pathname.startsWith("/api/webradios/stations/")) throw new Error("Unexpected station request: "+u.pathname);
    if(u.pathname==="/api/playlists/"+ids.playlist) {
      return json({id:ids.playlist,name:"Browser queue fixture",description:null,user_id:null,cover_url:null,is_public:true,tracks});
    }
    if(u.pathname.startsWith("/api/tracks/") && ids.tracks.some(id=>u.pathname.includes(id))) {
      const fixtureTrack=tracks.find(t=>u.pathname==="/api/tracks/"+t.id);
      if(fixtureTrack) return json(fixtureTrack);
    }
    // Synthetic tracks do not create fake listen history in the running instance.
    if(u.pathname==="/api/listen" || u.pathname==="/api/history" && request.method==="POST") return json({});
    return command("Fetch.continueRequest",{requestId});
  }
  chrome.intercept(intercept);
  await command("Fetch.enable",{patterns:[{urlPattern:"*"}]});
  await command("Page.addScriptToEvaluateOnNewDocument",{source:`
    if(location.origin===${q(origin)}) {
      if(!localStorage.getItem("soundtime_access_token")) {
        localStorage.setItem("soundtime_access_token",${q(tokens.access_token)});
        localStorage.setItem("soundtime_refresh_token",${q(tokens.refresh_token)});
      }
      localStorage.setItem("soundtime_lang","en");
      window.fixtureMedia=[];
      window.fixtureStarts=[];
      const play=HTMLMediaElement.prototype.play;
      HTMLMediaElement.prototype.play=function(...args) {
        if(!window.fixtureMedia.includes(this)) {
          window.fixtureMedia.push(this);
          this.addEventListener("playing",()=>window.fixtureStarts.push({url:this.currentSrc,position:this.currentTime}));
        }
        return play.apply(this,args);
      };
    }`});
  const navSlot='#workspace-directory a[href="/radio"] + a[href="/webradios"]';
  const radioPlay='article button[aria-label="Play Night Wave"]';
  const field=(label)=>`[...document.querySelectorAll("label")].find(el=>el.textContent.trim()===${q(label)})?.querySelector('input[role=switch]')`;
  const globalSwitch=field("Allow radio and podcasts on this instance"), personalSwitch=field("Show and play radio and podcasts");
  const witness=(path)=>`window.fixtureMedia?.find(el=>el.currentSrc.endsWith(${q(path)}) && !el.paused && el.readyState>=2)?.currentTime`;
  const check=s=>report.checks.push(s);
  const card=title=>`Array.from(document.querySelectorAll("main article h2")).some(h=>h.textContent.trim()===${q(title)})`;

  await viewport(false);
  await page("/webradios");
  await dom("desktop sidebar adjacency",`Boolean(${visible(navSlot)})`);
  await dom("initial listing",card("Night Wave"));
  await capture("desktop");
  check("desktop_radio_then_webradios");

  await viewport(true);
  await clickCss("#directory-toggle");
  await dom("mobile drawer adjacency",`Boolean(${visible(navSlot)})`);
  await capture("mobile");
  await clickCss(navSlot);
  await dom("mobile menu closed","!document.querySelector('[role=dialog]')");
  await dom("mobile station grid",card("Night Wave"));
  await capture("grid-mobile");
  await viewport(false);
  check("mobile_drawer_radio_then_webradios");

  await dom("station filters","!!document.querySelector('form input[name=name]')");
  await evaluate(`(()=>{const names=document.querySelector('input[name=name]'), tag=document.querySelector('input[name=tag]'); names.value='Wave'; tag.value='jazz'; names.form.requestSubmit();})()`,true);
  await dom("name and genre search","location.search.includes('name=Wave') && location.search.includes('tag=jazz') && "+card("Night Wave"));
  await clickCss('main nav a[href*="offset=30"]');
  await dom("second page preserves both filters",card("Night Wave Two"));
  assert.ok(report.directory.some(r=>r.name==="Wave" && r.tag==="jazz" && r.offset===30));
  assert.equal(report.clicks,0);
  assert.equal(report.media.length,0);
  check("filter_and_page_without_audio");

  await page("/playlists/"+ids.playlist);
  await clickCss("button.play-cell");
  const queuePath="/api/tracks/"+ids.tracks[0]+"/stream";
  await dom("decoded queue position",`(${witness(queuePath)})>1.25`);
  await clickCss(navSlot);
  await dom("station card",card("Night Wave"));
  const pausedAt=await evaluate(witness(queuePath));
  await clickCss(radioPlay);
  await dom("real live playback",`(${witness("/live.wav")})>0.35 && document.querySelector("section.live-bar [role=status]")?.textContent.trim()==='Live'`);
  const t0=await evaluate(witness("/live.wav")); await sleep(400);
  const t1=await evaluate(witness("/live.wav")); assert.ok(t1>t0+0.1,"Live media clock stalled");
  assert.equal(await evaluate(`window.fixtureMedia.filter(el=>!el.paused).length`),1);
  await capture("live");
  await clickCss("section.live-bar button[aria-label='Stop Night Wave']");
  await dom("queue restored paused",`!document.querySelector("section.live-bar") && !!document.querySelector('[data-testid="audio-player"] button.play[aria-label="Play"]') && window.fixtureMedia.every(el=>el.paused)`);
  const started=await evaluate("window.fixtureStarts.length");
  await clickCss('[data-testid="audio-player"] button.play');
  const resumed=await dom("resumed queue media event", `window.fixtureStarts.slice(${started}).find(e=>e.url.endsWith(${q(queuePath)}))`);
  assert.ok(resumed.position>=pausedAt-0.4, `Queue restarted at ${resumed.position}s after pausing at ${pausedAt}s`);
  report.resumed_from_seconds=resumed.position;
  await dom("same recording resumed at saved position",`(${witness(queuePath)})>=${pausedAt+0.12}`);
  await clickCss('[data-testid="audio-player"] button.skip:not(.previous)');
  await dom("second queue member retained",`(${witness('/api/tracks/'+ids.tracks[1]+'/stream')})>0.2`);
  await clickCss('[data-testid="audio-player"] button.play');
  assert.equal(report.clicks,1);
  report.live_clock_seconds=[t0,t1];
  report.resume_position_seconds=pausedAt;
  check("decoded_live_stop_resume_preserves_queue");

  await clickCss('main a[href*="show=123456"]');
  await dom("podcast show detail", `Boolean(document.querySelector('main a[href*="podcasts.apple.com"]'))`);
  assert.equal(report.media.filter(entry=>entry.source==="episode").length,0);
  await clickCss('main button[aria-label="Play Inside the sound booth"]');
  await dom("podcast audio is decoded",`(${witness("/episode.wav")})>0.35 && !!document.querySelector("section.episode-bar")`);
  assert.equal(await evaluate("window.fixtureMedia.filter(el=>!el.paused).length"),1);
  await clickCss('section.episode-bar button[aria-label="Pause Inside the sound booth"]');
  await dom("episode can pause","window.fixtureMedia.every(el=>el.paused)");
  await clickCss('section.episode-bar button[aria-label="Play Inside the sound booth"]');
  await dom("episode can resume",`(${witness("/episode.wav")})>0.45`);
  await capture("podcast");
  check("episode_selection_decodes_and_pauses");
  await click(personalSwitch,"personal switch");
  await dom("personal opt-out",`!(${personalSwitch})?.checked && !document.querySelector('form input[name=name]') && !(${visible(navSlot)})`);
  await dom("opt-out closes podcast",'!document.querySelector("section.episode-bar") && window.fixtureMedia.every(el=>el.paused)');
  assert.equal((await api("/webradios/status")).user_enabled,false);
  const personalLimit=report.directory.length;
  await page("/webradios?name=Wave&tag=jazz");
  await dom("personal preference after reload",`!!(${personalSwitch}) && !(${personalSwitch}).checked`);
  assert.equal(report.directory.length,personalLimit);
  await page("/settings");
  await click(personalSwitch,"restore radios from settings");
  await dom("user navigation returns",`Boolean(${visible(navSlot)})`);
  check("saved_personal_choice_and_deep_link");

  await page("/admin");
  const settingsTab='[...document.querySelectorAll("main button")].find(b=>b.textContent.trim()==="Settings" && b.getClientRects().length)';
  await click(settingsTab,"admin Settings tab");
  await click(globalSwitch,"admin global switch");
  await dom("global switch off",`!!(${globalSwitch}) && !(${globalSwitch}).checked && !(${visible(navSlot)})`);
  assert.equal((await api("/webradios/status")).instance_enabled,false);
  const disabledLimit=report.directory.length;
  await page("/webradios?name=global-disabled");
  await dom("global denial on direct link",`(${personalSwitch})?.disabled===true && !document.querySelector('input[name=name]') && !(${visible(navSlot)})`);
  assert.equal(report.directory.length,disabledLimit);
  await capture("disabled");
  check("instance_choice_hides_navigation_and_blocks_deep_link");
}

let chrome, profile, initial, failure;
try {
  const tokens=await login();
  token=tokens.access_token;
  initial=await api("/webradios/status");
  if(!initial.instance_enabled) await api("/admin/settings/webradios_enabled","PUT",{value:"true"});
  if(!initial.user_enabled) await api("/webradios/preference","PUT",{enabled:true});
  profile=await mkdtemp(join(artifacts ?? tmpdir(),".soundtime-webradios-"));
  chrome=openChrome(profile);
  await chrome.attach();
  await run(chrome,tokens);
} catch(error) { failure=error; }
finally {
  if(chrome) await chrome.close();
  if(profile) await rm(profile,{recursive:true,force:true});
  try {
    if(initial) {
      const current=await api("/webradios/status");
      if(current.instance_enabled!==initial.instance_enabled) await api("/admin/settings/webradios_enabled","PUT",{value:String(initial.instance_enabled)});
      if(current.user_enabled!==initial.user_enabled) await api("/webradios/preference","PUT",{enabled:initial.user_enabled});
      assert.deepEqual(await api("/webradios/status"),initial,"Settings restoration");
      report.settings_restored=true;
    }
  } catch(error) { failure??=error; report.settings_restored=false; }
  if(failure) report.error=failure.message;
  if (piped) console.log(JSON.stringify({report,screenshots}));
  else await writeFile(join(artifacts,"webradios-browser.json"),JSON.stringify(report,null,2)+"\n");
}
if(failure) throw failure;
if (!piped) console.log(JSON.stringify({checks:report.checks,screenshots:report.screenshots,artifacts}));
