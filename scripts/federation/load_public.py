"""Observe B's public read tier and load a disposable database as a fenced owner."""
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import json
import os
import re
import subprocess
import threading
import time
from uuid import uuid4

from scenario import require


def sql(compose, project, statement, *, timeout=30):
    return compose(project, "exec", "-T", "db-b", "psql", "-XqAt", "-v", "ON_ERROR_STOP=1",
                   "-U", "soundtime", "-d", "soundtime", "-c", statement, timeout=timeout).decode().strip()


def prepare(compose, seed, dest):
    project = os.environ["SOUNDTIME_TEST_COMPOSE_PROJECT"]
    require(re.fullmatch("st-fed-[0-9a-f]{16}", project) and os.environ.get("SOUNDTIME_TEST_PUBLIC_LOAD") == "1",
            "Only the disposable public load project can be initialized")
    require(not compose(project, "ps", "-q", "backend-b").strip(), "Public worker already started")
    require(not sql(compose, project, "SELECT 1 FROM federation_worker_lease"), "Database has an existing owner")
    holder = str(uuid4())
    generation = int(sql(compose, project, f"SELECT federation_claim_worker('{holder}')"))
    owner = f"st/{generation}/{holder}"
    stop = threading.Event()
    condition = f"singleton AND generation={generation} AND holder='{holder}' AND expires_at>clock_timestamp()"

    def heartbeat():
        renewals = 0
        while not stop.wait(3):
            row = sql(compose, project, "UPDATE federation_worker_lease "
                      "SET expires_at=clock_timestamp()+interval '15 seconds' WHERE " + condition
                      + " RETURNING generation", timeout=5)
            require(row == str(generation), "Public index fixture lost its lease")
            renewals += 1
        return renewals

    started = time.monotonic()
    with ThreadPoolExecutor(max_workers=1) as executor:
        renewing = executor.submit(heartbeat)
        try:
            counts = seed(project, 298, 1000, owner)
        finally:
            stop.set()
        renewals = renewing.result()
    require(sql(compose, project, "UPDATE federation_worker_lease SET expires_at=clock_timestamp() WHERE "
                + condition + " RETURNING generation") == str(generation), "Could not release fixture lease")
    Path(dest).write_text(json.dumps({"project": project, "seed": counts, "generation": generation,
        "lease_renewals": renewals, "elapsed_s": round(time.monotonic()-started, 3)}, indent=2)+"\n")


def containers(compose, project, roles, public):
    ids = {}
    for role in roles:
        lines = compose(project, "ps", "-q", role).decode().splitlines()
        require(len(lines) == 1, f"Expected one running {role}")
        ids[lines[0][:12]] = role
    readers = {}
    if public:
        lines = compose(project, "ps", "-q", "catalog-http").decode().splitlines()
        require(len(lines) == 2, "Expected two running catalogue readers")
        template = '{{index .Config.Labels "com.docker.compose.container-number"}} {{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}'
        for container in lines:
            number, address = subprocess.check_output(["docker", "inspect", "-f", template, container],
                                                       text=True, timeout=10).strip().split()
            require(number in ("1", "2") and re.fullmatch(r"[0-9.]+",address), "Unexpected catalogue reader network")
            role = "catalog-http-" + number
            ids[container[:12]] = role
            readers[address+":8080"] = role
    return ids, readers


def nginx_settings(compose, project):
    source = compose(project, "exec", "-T", "proxy-b", "nginx", "-T").decode()
    result = {}
    for directive in ("worker_processes", "worker_connections", "worker_rlimit_nofile",
                      "keepalive", "proxy_buffering", "proxy_http_version"):
        result[directive] = re.findall(r"^\s*" + directive + r"\s+([^;#\n]+);", source, re.MULTILINE)
    result["catalogue_least_conn_enabled"] = "least_conn;" in source and "upstream catalogue_readers" in source
    processes = compose(project, "exec", "-T", "proxy-b", "ps", "-o", "pid,args").decode().splitlines()
    pids = [row.split()[0] for row in processes if re.match(r"^\s*\d+\s+nginx: worker process",row)]
    require(pids, "No live Nginx worker processes")
    result["active_workers"] = len(pids)
    paths = [f"/proc/{pid}/limits" for pid in pids]
    rlimits = compose(project, "exec", "-T", "proxy-b", "awk",
                      '/^Max open files/ {print $4, $5}', *paths).decode().splitlines()
    result["live_worker_nofile_soft_hard"] = sorted(set(rlimits))
    result["postgres_max_connections"] = int(sql(compose, project, "SHOW max_connections"))
    return result


def connections(compose, project, readers):
    # Counters come from the proxy's TCP namespace. Chrom(e/ium) and HTTP readers both use this ingress.
    data = compose(project, "exec", "-T", "proxy-b", "cat", "/proc/net/tcp", "/proc/net/tcp6").decode()
    rows = [row.split() for row in data.splitlines() if len(row.split()) > 4 and row.split()[3] == "01"]
    result = {"inbound_established": sum(r[1].endswith(":0050") for r in rows),
              "upstream_established": sum(r[2].upper().endswith(":1F90") for r in rows)}
    if readers:
        db_activity = sql(compose, project,
            "SELECT host(client_addr)||' '||count(*) FROM pg_stat_activity "
            "WHERE client_addr IS NOT NULL AND datname=current_database() GROUP BY client_addr")
        by_address = {ip: int(n) for ip,n in (line.split() for line in db_activity.splitlines())}
        result["database_sessions_per_reader"] = {
            role: by_address.get(address.split(":")[0],0) for address,role in readers.items()}
    return result


def routed_searches(compose, project, readers):
    lines = compose(project, "exec", "-T", "proxy-b", "cat", "/tmp/soundtime-pilot-readers.log").decode().splitlines()
    counted = {name: 0 for name in readers.values()}
    unexpected = 0
    for row in lines:
        fields = row.split()
        if len(fields) == 2 and fields[0] in readers and fields[1] == "200":
            counted[readers[fields[0]]] += 1
        else:
            unexpected += 1
    return {**counted, "retries_errors_or_other_upstreams": unexpected}
