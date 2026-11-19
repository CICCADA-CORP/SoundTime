import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import AdminPage from "./+page.svelte";

vi.mock("$lib/stores/auth.svelte", () => ({ getAuthStore: () => ({isAdmin:true, loading:false}) }));
afterEach(() => vi.unstubAllGlobals());

function openPeers(explicit: boolean) {
  setLocale("en");
  let followed = false;
  let reject = false;
  const fetch = vi.fn(async (url: string, options: RequestInit) => {
    if (url === "/api/p2p/status") return Response.json({enabled:true, explicit_catalog:explicit, node_id:"here",
      direct_addresses:1, peer_count:1, online_peer_count:1});
    if (url === "/api/admin/p2p/peers" && options.method === "POST") return Response.json({message:"Peer saved"});
    if (url === "/api/admin/p2p/peers") return Response.json([
      {node_id:"peer-one",name:"Studio",catalog_followed:explicit ? followed : null, is_online:true, track_count:4},
      {node_id:"old-peer",name:"Archive",catalog_followed:null, is_online:false, track_count:3},
    ]);
    if (url === "/api/admin/p2p/peers/peer-one/subscription") {
      if (reject) return Response.json({message:"Peer blocked"}, {status:403});
      followed = JSON.parse(String(options.body)).followed;
      return Response.json({message:"Saved"});
    }
    return Response.json(null);
  });
  vi.stubGlobal("fetch",fetch);
  render(AdminPage);
  return {fetch, open:()=>fireEvent.click(screen.getByRole("button",{name:"P2P Status"})), reject:()=>reject=true};
}

it("follows and unfollows, refreshes the authoritative result and surfaces a policy denial",async()=>{
  const page=openPeers(true);
  await page.open();
  const follow=await screen.findByRole("button",{name:"Follow catalogue · Studio"});
  expect(screen.queryByRole("button",{name:"Follow catalogue · Archive"})).toBeNull();
  await fireEvent.click(follow);
  await fireEvent.click(await screen.findByRole("button",{name:"Unfollow catalogue · Studio"}));
  await screen.findByRole("button",{name:"Follow catalogue · Studio"});
  page.reject();
  await fireEvent.click(screen.getByRole("button",{name:"Follow catalogue · Studio"}));
  expect((await screen.findByRole("alert")).textContent).toContain("Peer blocked");
  const requests=page.fetch.mock.calls.filter(([,opts])=>opts.method==="PUT");
  expect(requests.map(([,opts])=>JSON.parse(String(opts.body)))).toEqual([{followed:true},{followed:false},{followed:true}]);
  expect(page.fetch.mock.calls.filter(([url,opts])=>url==="/api/admin/p2p/peers" && !opts.method)).toHaveLength(4);
});

it.each([true,false])("keeps historical manual-add semantics and enables presence-only when negotiated (%s)",async(explicit)=>{
  const page=openPeers(explicit);
  await page.open();
  const add=await screen.findByRole("button",{name:"Add peer"});
  const input=screen.getByRole("textbox");
  const checkbox=screen.queryByRole("checkbox",{name:"Follow catalogue"});
  if (explicit) {
    expect(checkbox).toBeChecked();
    await fireEvent.click(checkbox!);
  } else {
    expect(checkbox).toBeNull();
    expect(screen.queryByRole("button",{name:"Follow catalogue · Studio"})).toBeNull();
  }
  await fireEvent.input(input,{target:{value:"another-peer"}});
  await fireEvent.click(add);
  await waitFor(()=>expect(page.fetch.mock.calls.filter(([,opts])=>opts.method==="POST")).toHaveLength(1));
  const body=page.fetch.mock.calls.find(([,opts])=>opts.method==="POST")![1].body;
  expect(JSON.parse(String(body))).toEqual(explicit ? {node_id:"another-peer",follow_catalog:false} : {node_id:"another-peer"});
  if (explicit) await waitFor(()=>expect(checkbox).toBeChecked());
});
