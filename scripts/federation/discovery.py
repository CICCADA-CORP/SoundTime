"""Verify private discovery, authenticated LAN/listing candidates and a durable ban."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
from urllib.error import HTTPError, URLError

from scenario import Instance, require, wait_for, wav_fixture


def run(args, report):
    a, b, c = [Instance(name, getattr(args, name.lower())) for name in "ABC"]
    project = os.environ["SOUNDTIME_TEST_COMPOSE_PROJECT"]
    require(re.fullmatch(r"st-fed-[a-f0-9]{16}", project), "Expected a disposable discovery project")
    here = Path(__file__).parent
    cmd = ["docker", "compose", "--env-file", "/dev/null", "-p", project, "-f", str(here/"compose.yml"),
           "-f", str(here/"discovery.yml")]

    def compose(*args, data=None):
        return subprocess.check_output([*cmd, *args], input=data, text=True, timeout=45)

    def restart(node):
        expected = node.node_id
        compose("restart", "-t", "3", "backend-" + node.name.lower())

        def ready():
            try:
                with node.open("/p2p/status") as response:
                    return json.load(response).get("node_id") == expected
            except (URLError, OSError):
                return False
        wait_for("restarted " + node.name, ready, timeout=60)

    def online(node, id):
        return any(row["node_id"] == id and row["is_online"] for row in node.call("/admin/p2p/peers"))

    def requests(method, path, status):
        return compose("logs", "--no-color", "listing").count(f'"{method} {path} HTTP/1.1" {status}')

    def listing_reads():
        return requests("GET", "/api/nodes", 200)

    for node in (a,b,c):
        require(not node.call("/setup/status")["has_admin"], "Expected fresh databases")
        node.account("admin" + node.name.lower(), admin=True)
        node.identify()
    b.call("/admin/settings/instance_private", method="PUT", body={"value":"true"})
    try:
        with b.open("/p2p/status", token=""):
            raise AssertionError("Private instance exposed its public status")
    except HTTPError as error:
        with error:
            require(error.code == 401, "Unexpected private status response")
    require(not b.call("/nodeinfo", token="").get("p2p_node_id"), "Private NodeId leaked via public nodeinfo")
    title = "Discoveryonlyfixture"
    a.upload(title, wav_fixture(24000, 37))
    restart(b)
    wait_for("B reads without registering at the rendezvous",
             lambda: requests("GET", "/api/p2p/rendezvous", 200), timeout=60)
    restart(a)
    wait_for("passive LAN discovery of A by B", lambda: online(b, a.node_id), timeout=60)
    require(not online(c, a.node_id), "C already knew A before the listing supplied it")
    report["private_LAN_peer"] = a.node_id
    fixture = {"total":1, "nodes":[{"domain":"fixture.invalid","is_online":True,
                                    "p2p_enabled":True,"p2p_node_id":a.node_id}]}
    compose("exec", "-T", "listing", "sh", "-c",
            "mkdir -p /usr/share/nginx/html/api && cat > /usr/share/nginx/html/api/nodes", data=json.dumps(fixture))
    restart(c)
    wait_for("C's signed registration", lambda: a.call("/p2p/rendezvous", token="") == [c.node_id], timeout=45)
    wait_for("C authenticates the directory candidate without a subscription",
             lambda: listing_reads() and online(c, a.node_id), timeout=60)
    require(a.node_id not in [row["id"] for row in c.call("/p2p/network-graph", token="")["nodes"]]
            and a.node_id in [row["id"] for row in c.call("/admin/p2p/network-graph")["nodes"]],
            "Network graph disregarded identity redistribution consent")
    require(not b.search(title) and not c.search(title), "Discovery imported an unfollowed catalogue")
    require(all(node.telemetry()["catalog_entries_received"] == 0 for node in (b,c)),
            "Discovery transmitted catalogue entries")
    report["public_listing_peer"] = a.node_id
    block = c.call("/admin/blocked-domains", method="POST", body={"domain":a.node_id})
    before = listing_reads()
    restart(c)
    wait_for("directory replay after restarting the banned reader", lambda: listing_reads() > before, timeout=60)
    reply = c.call("/admin/p2p/peers/" + a.node_id + "/ping", method="POST")["message"]
    require("blocked" in reply.lower() and not online(c,a.node_id), "Blocked directory peer was readmitted")
    require(any(item["id"] == block["id"] for item in c.call("/admin/blocked-domains")),
            "Peer ban did not survive restart")
    require(a.call("/p2p/rendezvous", token="") == [c.node_id], "Private reader appeared in rendezvous")
    require('"POST /api/announce ' not in compose("logs", "--no-color", "listing"), "A listing reader announced itself")
    report.update({"persisted_ban":a.node_id,"signed_directory_members":[c.node_id],
                   "rendezvous_reads":requests("GET", "/api/p2p/rendezvous", 200),
                   "rendezvous_registrations":requests("POST", "/api/p2p/rendezvous", 200),
                   "listing_gets":listing_reads(),"status":"accepted"})


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    for arg in ("a","b","c","report"):
        parser.add_argument("--"+arg, required=True)
    parser.add_argument("--mode", choices=["discovery"], required=True)
    args = parser.parse_args()
    report = {"mode":"discovery","status":"failed"}
    try:
        run(args, report)
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        Path(args.report).write_text(json.dumps(report, indent=2)+"\n")
