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


def sql(compose, project, statement, *, timeout=30, site="B"):
    require(site in ("A", "B", "C"), "Unknown fixture database")
    return compose(project, "exec", "-T", "db-"+site.lower(), "psql", "-XqAt", "-v", "ON_ERROR_STOP=1",
                   "-U", "soundtime", "-d", "soundtime", "-c", statement, timeout=timeout).decode().strip()


def wait_for_journals(compose, project, observation, timeout):
    # inventory::source::head reads federation_event_bootstrap.complete. Its job
    # can start only after the publication projection reached its own checkpoint.
    query = """SELECT json_build_object(
        'projection_complete',p.complete,'last_projected_track',p.last_track_id,
        'journal_complete',j.complete,'last_journal_track',j.last_track)
        FROM federation_backfill p CROSS JOIN federation_event_bootstrap j
        WHERE p.singleton AND j.singleton"""
    start = time.monotonic()
    deadline = start + timeout
    sites = {}
    observation.update(timeout_s=timeout, polls=0, sites=sites)
    while True:
        observation["polls"] += 1
        for site in ("A", "B", "C"):
            if sites.get(site, {}).get("journal_complete") is True:
                continue
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            raw = sql(compose, project, query, site=site, timeout=min(10, remaining))
            require(raw, f"{site} has no historical publication checkpoint")
            sites[site] = json.loads(raw)
        observation["elapsed_s"] = round(time.monotonic()-start, 3)
        if len(sites) == 3 and all(s["journal_complete"] is True for s in sites.values()):
            return
        remaining = deadline - time.monotonic()
        require(remaining > 0, f"Historical publication journals still preparing: {sites}")
        time.sleep(min(2,remaining))


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
    if os.environ.get("SOUNDTIME_TEST_PUBLIC_LOAD") == "1":
        def pool(service, replica=1):
            values = compose(project, "exec", "-T", "--index", str(replica), service, "printenv",
                             "DB_MIN_CONNECTIONS", "DB_MAX_CONNECTIONS").decode().splitlines()
            minimum, maximum = map(int, values)
            return {"minimum": minimum, "maximum": maximum}

        # The Rust binary is PID 1 in the public worker and reader images.
        # Inspect the live process in each replica; docker inspect only gives
        # the requested configuration and Nginx has a separate descriptor limit.
        rust_limits = {}
        for service, replica in [("backend-b", None), ("catalog-http", 1), ("catalog-http", 2)]:
            selected = ["--index", str(replica)] if replica else []
            role = service if replica is None else f"{service}-{replica}"
            command = (project, "exec", "-T", *selected, service)
            # Linux truncates /proc/1/comm to 15 bytes ("soundtime-serve").
            executable = compose(*command, "readlink", "/proc/1/exe").decode().strip()
            require(executable == "/app/soundtime-server",
                    f"Unexpected executable when reading descriptor budget for {role}: {executable}")
            fields = compose(*command, "awk", '/^Max open files/ {print $4,$5}',
                             "/proc/1/limits").decode().split()
            require(len(fields) == 2, f"Missing descriptor limits on {role}: {fields}")
            soft, hard = fields
            require(all(n == "unlimited" or n.isdigit() and int(n) >= 8192 for n in (soft, hard)),
                    f"Insufficient public Rust descriptor limit on {role}: soft={soft} hard={hard}")
            rust_limits[role] = {"executable": executable, "soft": soft, "hard": hard}
        result["rust_process_nofile"] = rust_limits
        worker = pool("backend-b")
        readers = [pool("catalog-http", replica) for replica in (1, 2)]
        limits = json.loads(sql(compose, project, """SELECT json_build_object(
            'reserved_slots',current_setting('superuser_reserved_connections')::int +
                current_setting('reserved_connections')::int,
            'parallel_workers_per_gather',current_setting('max_parallel_workers_per_gather')::int,
            'lease_sessions',(SELECT count(*) FROM pg_stat_activity
                              WHERE datname=current_database() AND application_name='soundtime/worker-lease'))"""))
        require(limits["lease_sessions"] == 1, "Worker needs its own live lease session")
        client_limit = 1 + worker["maximum"] + sum(reader["maximum"] for reader in readers)
        remaining = result["postgres_max_connections"] - limits["reserved_slots"] - client_limit
        require(remaining >= 1, "The configured pools exhaust PostgreSQL before probes and maintenance")
        shm = compose(project, "exec", "-T", "db-b", "df", "-Pk", "/dev/shm").decode().splitlines()[-1].split()
        require(shm[-1] == "/dev/shm", "PostgreSQL shared memory mount is missing")
        result["database_budget"] = {**limits, "worker_serving_pool": worker, "catalogue_pools": readers,
                                     "application_pool_ceiling": client_limit, "remaining_unreserved_slots": remaining,
                                     "shared_memory_mount_bytes": int(shm[1])*1024}
    return result


def connections(compose, project, readers):
    # Counters come from the proxy's TCP namespace. Chrom(e/ium) and HTTP readers both use this ingress.
    data = compose(project, "exec", "-T", "proxy-b", "cat", "/proc/net/tcp", "/proc/net/tcp6").decode()
    rows = [row.split() for row in data.splitlines() if len(row.split()) > 4 and row.split()[3] == "01"]
    result = {"inbound_established": sum(r[1].endswith(":0050") for r in rows),
              "upstream_established": sum(r[2].upper().endswith(":1F90") for r in rows)}
    if readers:
        try:
            db_activity = sql(compose, project, """SELECT json_build_array(ip,role,count(*))
                FROM (SELECT host(client_addr) AS ip, CASE
                    WHEN application_name LIKE 'st/%' THEN 'worker'
                    WHEN application_name='soundtime/worker-lease' THEN 'lease'
                    WHEN application_name='soundtime/catalog-http' THEN 'catalogue' ELSE 'other' END AS role
                    FROM pg_stat_activity WHERE datname=current_database() AND backend_type='client backend') s
                GROUP BY ip,role""")
            by_role, by_address = {}, {}
            for line in db_activity.splitlines():
                ip, role, count = json.loads(line)
                by_role[role] = by_role.get(role, 0) + count
                if role == "catalogue":
                    by_address[ip] = count
            result["database_sessions_by_role"] = by_role
            result["database_sessions_per_reader"] = {
                role: by_address.get(address.split(":")[0], 0) for address, role in readers.items()}
        except subprocess.SubprocessError as error:
            result["database_sampling_error"] = str(error)
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
