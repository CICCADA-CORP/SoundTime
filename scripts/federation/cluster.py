"""Exercise a worker takeover, a fenced write and rollback on one isolated instance."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import time
from urllib.error import URLError
from urllib.request import urlopen

from scenario import Instance, probe_stream, require, wait_for, wav_fixture


def exercise(args, report):
    project = os.environ["SOUNDTIME_TEST_COMPOSE_PROJECT"]
    require(re.fullmatch(r"st-fed-[a-f0-9]{16}", project), "expected a disposable project")
    root = Path(__file__).parent
    cli = ["docker", "compose", "--env-file", "/dev/null", "-p", project, "-f", str(root/"compose.yml"),
           "-f", str(root/"cluster.yml")]

    def compose(*parts):
        return subprocess.check_output([*cli, *parts], text=True, timeout=90).strip()

    def sql(statement):
        return compose("exec","-T","db-a","psql","-XAtq","-v","ON_ERROR_STOP=1","-U","soundtime",
                       "-d","soundtime","-c",statement)

    def url(service, index=1):
        address = compose("port","--index",str(index),service,"8080")
        require(re.fullmatch(r"127\.0\.0\.1:\d+",address), "expected a loopback endpoint")
        return "http://"+address

    def healthy(base):
        try:
            with urlopen(base+"/healthz",timeout=1) as answer:
                return answer.status==200
        except (URLError, TimeoutError, OSError):
            return False

    a, b = Instance("A",args.a), Instance("B",args.b)
    admin = a.account("owner",admin=True)
    b.account("onb",admin=True)
    a.identify()
    b.identify()
    data = wav_fixture(1024 * 1024,9)
    track = a.upload("Restoredidentity",data)
    playlist = a.playlist(admin,"Preserved after failover",[track])
    replica_ids = []
    for index in (1,2):
        replica = Instance(f"reader{index}",url("catalog-http",index))
        replica_ids.append(wait_for("catalogue replica",lambda: replica.search("Restoredidentity")))
    require(replica_ids == [track,track], "two catalogue replicas disagreed")
    a.call("/admin/blocked-domains",method="POST",body={"domain":b.node_id})
    a.call("/tracks/"+track,method="DELETE")
    require(probe_stream(a,track,data)==410,"withdrawal on active owner")
    old = sql("SELECT 'st/'||generation||'/'||holder FROM federation_worker_lease")
    require(re.fullmatch(r"st/\d+/[a-f0-9-]{36}",old),"invalid generation")
    compose("up","-d","--no-build","standby-a")
    standby = Instance("standby",url("standby-a"))
    standby.token = a.token
    require(not healthy(standby.url),"standby served before acquiring the identity")
    # Kill only the named test worker. Its SQL lease remains until database time expires.
    compose("kill","-s","KILL","backend-a")
    started = time.monotonic()
    require(wait_for("standby takeover",lambda: healthy(standby.url),timeout=45),"standby offline")
    elapsed = time.monotonic()-started
    standby.identify()
    new = sql("SELECT generation FROM federation_worker_lease")
    require(int(new)>int(old.split("/")[1]) and standby.node_id==a.node_id,"identity or generation changed incorrectly")
    sql("BEGIN; SET LOCAL application_name='"+old+"'; DO $$ BEGIN "
        "BEGIN UPDATE publication_offers SET expires_at=expires_at WHERE false; "
        "RAISE EXCEPTION 'stale worker was accepted'; EXCEPTION WHEN SQLSTATE '55000' THEN NULL; END; END $$; ROLLBACK")
    require(probe_stream(standby,track,data)==410, "rollback resurrected a withdrawal")
    rows = standby.positions(playlist)
    require(len(rows)==1 and rows[0]["track_id"]==track and rows[0]["availability"]=="withdrawn","lost saved position")
    require(any(p["domain"]==b.node_id for p in standby.call("/admin/blocked-domains")), "rollback lost ban")
    require(not standby.telemetry()["range_streaming"], "expected limited transport on replacement")
    report.update(track=track, reader_results=replica_ids, node_id=a.node_id, previous_generation=int(old.split("/")[1]),
                  generation=int(new), recovery_seconds=elapsed, withdrawn_http_status=410, retained_position=rows[0]["position"])
    report["status"]="accepted"


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    for option in ("a","b","c","mode"): parser.add_argument("--"+option,required=True)
    parser.add_argument("--report",type=Path,required=True)
    args=parser.parse_args()
    report={"mode":args.mode,"status":"failed"}
    try:
        exercise(args,report)
    finally:
        args.report.write_text(json.dumps(report,indent=2)+"\n")
