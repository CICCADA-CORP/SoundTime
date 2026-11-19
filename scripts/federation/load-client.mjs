// The runner supplies fixed disposable loopback addresses and a short-lived listener credential via stdin.
import http from "node:http";
import { setMaxListeners } from "node:events";
import { setTimeout as delay } from "node:timers/promises";
import { monitorEventLoopDelay, performance } from "node:perf_hooks";
import { createInterface } from "node:readline";
import process from "node:process";

const lines=createInterface({input:process.stdin});
const [line]=await Array.fromAsync(lines);
const cfg=JSON.parse(line);
const base=new URL(cfg.base);
if(base.hostname!=="127.0.0.1" || base.protocol!=="http:" || !/^st-fed-[0-9a-f]{16}$/.test(process.env.SOUNDTIME_TEST_COMPOSE_PROJECT||""))
  throw Error("Load clients require the disposable local Compose project");
if (!["load-smoke", "load-full"].includes(cfg.profile) || !cfg.tokens?.length)
  throw Error("Missing load profile or listener credentials");
const full=cfg.profile==="load-full";
const clients=full?1000:6, rate=full?100:3, duration=full?90000:15000, ramp=full?20000:2000;
const readRate=32*1024;
const agent=new http.Agent({keepAlive:true,maxSockets:clients+Math.max(rate*3,16)+10});
const signal=new AbortController(); setMaxListeners(0, signal.signal);
const loop=monitorEventLoopDelay({resolution:10}); loop.enable();
const start=performance.now(), steady=start+ramp, end=steady+duration, startedCpu=process.cpuUsage(), startedLoop=performance.eventLoopUtilization();
const metrics={search:[],search_total:[],first_byte:[],first_byte_steady:[],dispatch_lag:[],samples:[],errors:{},search_due:0,
  search_started:0,search_completed:0,search_omitted:0,body_bytes:0,media_starts:0,interruptions:0};
let active=0, sessions=0, queries=0, bytes=0, issued=0, steadyActivePeak=0;
const pending=new Set();
const log=(data)=>console.log(JSON.stringify(data));
function err(key){metrics.errors[key]=(metrics.errors[key]||0)+1;}
function record(list,v){if(performance.now()>=steady)list.push(v);}
async function get(path,listener,headers={}){
  return new Promise((resolve,reject)=>{
    const req=http.get(new URL(path,base),{agent,signal:signal.signal,headers:{...headers,Authorization:"Bearer "+cfg.tokens[listener%cfg.tokens.length]}},resolve);
    req.setTimeout(15000,()=>req.destroy(Error("inactivity_timeout")));
    req.on("error",reject);
  });
}
async function search(n,scheduled){
  queries++;
  const begin=performance.now();
  const origin=1+n%cfg.virtual_catalogues;
  const term=n%4===0?"Melody"+String(n%20).padStart(2,"0"):"Quartz"+String(origin).padStart(4,"0");
  try{
    const res=await get("/api/search?q="+term+"&include_p2p=false&limit=10",n);
    let body="",size=0;
    for await(const chunk of res){size+=chunk.length;if(size>2*1024*1024)throw Error("response_too_large");body+=chunk;}
    if(res.statusCode!==200 || !JSON.parse(body).tracks?.length)throw Error("invalid_search_"+res.statusCode);
    if(scheduled>=steady){metrics.search_completed++;metrics.search.push(performance.now()-begin);metrics.search_total.push(performance.now()-scheduled);}
  }catch(e){if(!signal.signal.aborted)err("search_"+(e.code||e.message));}
  finally{queries--;}
}
async function session(i){
  await delay(i*ramp/clients);
  // Coincident hot ranges, offset listeners, and the remaining distinct audio objects.
  const group=i%4;
  const tr=cfg.media[group<2?0:(i%cfg.media.length)];
  const beginOffset=group===0?0:65536*((i*37)%Math.floor(tr.size/(2*65536)));
  let offset=beginOffset;
  sessions++;
  try{
    while(!signal.signal.aborted){
      const began=performance.now();
      let counted=false, res;
      try{
        res=await get("/api/tracks/"+tr.id+"/stream",i,{Range:"bytes="+offset+"-"});
        if(res.statusCode!==206 || res.headers["content-range"]!=="bytes "+offset+"-"+(tr.size-1)+"/"+tr.size)
          throw Error("media_status_"+res.statusCode);
        ++active;counted=true;metrics.media_starts++;steadyActivePeak=Math.max(steadyActivePeak,active);
        let delivered=0;
        for await(const chunk of res){
          if(delivered===0){metrics.first_byte.push(performance.now()-began);record(metrics.first_byte_steady,performance.now()-began);}
          // Check the fixture pattern against absolute offsets, including after a reopen.
          for(let j=0;j<chunk.length;j+=4096){
            const position=offset+delivered+j;
            if(position>=44 && chunk[j]!==(((position-44)%256)*tr.seed%127)+64)throw Error("incorrect_audio");
          }
          delivered+=chunk.length;bytes+=chunk.length;
          if(performance.now()>=steady)metrics.body_bytes+=chunk.length;
          const wait=began+delivered/readRate*1000-performance.now();
          if(wait>0)await delay(wait,undefined,{signal:signal.signal});
        }
        if(offset+delivered!==tr.size)throw Error("short_audio");
        offset=0;
      }catch(e){
        if(!signal.signal.aborted){metrics.interruptions++;err("media_"+(e.code||e.message));await delay(250);}
      }finally{res?.destroy();if(counted)--active;}
    }
  }finally{sessions--;}
}
const tasks=Array.from({length:clients},(_,i)=>session(i));
const dispatch=setInterval(()=>{
  const now=performance.now();
  while(start+issued*1000/rate<=now && start+issued*1000/rate<end){
    const n=issued++, at=start+n*1000/rate;
    if(at>=steady){metrics.search_due++;metrics.dispatch_lag.push(now-at);}
    if(queries>=Math.max(rate*3,16)||now-at>1000){
      if(at>=steady)metrics.search_omitted++;
      continue;
    }
    if(at>=steady)metrics.search_started++;
    const p=search(n,at);pending.add(p);p.finally(()=>pending.delete(p));
  }
},5);
const samples=setInterval(()=>{
  metrics.samples.push({ms:Math.round(performance.now()-start),media_sessions:sessions,open_media_responses:active,
    search_inflight:queries,interruptions:metrics.interruptions,errors:{...metrics.errors},
    rss_bytes:process.memoryUsage().rss,media_bytes:bytes});
},1000);
await delay(ramp);
log({phase:"steady",clients,search_per_second:rate,duration_s:duration/1000});
await delay(Math.max(1,end-performance.now()));
clearInterval(dispatch);clearInterval(samples);
signal.abort();agent.destroy();await Promise.allSettled([...tasks,...pending]);loop.disable();
function quantiles(values){
  values.sort((a,b)=>a-b);
  const p=(q)=>values.length?Number(values[Math.max(0,Math.ceil(values.length*q)-1)].toFixed(2)):null;
  return {samples:values.length,p50:p(.5),p95:p(.95),p99:p(.99)};
}
const steadySamples=metrics.samples.filter(s=>s.ms>=ramp+1000);
const load={dispatched_search_per_second:metrics.search_started/(duration/1000),
  completed_search_per_second:metrics.search_completed/(duration/1000),target_readers:clients,
  observed_open_media:quantiles(steadySamples.map(s=>s.open_media_responses)),
  samples_at_target:steadySamples.filter(s=>s.open_media_responses===clients).length,
  steady_samples:steadySamples.length,open_media_peak:steadyActivePeak};
const host={rss_peak_bytes:Math.max(0,...metrics.samples.map(s=>s.rss_bytes)),cpu:process.cpuUsage(startedCpu),
 event_loop_utilization:performance.eventLoopUtilization(startedLoop).utilization,
 event_loop_delay_p99_ms:loop.percentile(99)/1e6,search_dispatch_lag_ms:quantiles(metrics.dispatch_lag)};
log({phase:"complete",profile:cfg.profile,load,generator:host,search_http_ms:quantiles(metrics.search),
 search_scheduled_to_response_ms:quantiles(metrics.search_total),first_audio_byte_ms:quantiles(metrics.first_byte),steady_first_audio_byte_ms:quantiles(metrics.first_byte_steady),
 omissions:metrics.search_omitted,search_due:metrics.search_due,search_started:metrics.search_started,
 search_completed:metrics.search_completed,media_starts:metrics.media_starts,interruptions:metrics.interruptions,
 useful_bytes:metrics.body_bytes,errors:metrics.errors,samples:metrics.samples});
