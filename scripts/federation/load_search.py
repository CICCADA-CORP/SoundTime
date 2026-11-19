"""Check B's indexed search path and sample the same prepared SQL as the HTTP readers."""
import json
from pathlib import Path
import subprocess
import time

from load_public import sql
from scenario import require


QUERIES = Path(__file__).resolve().parents[2] / "backend/crates/soundtime-server/src/api/search"


def wait_for_index(compose, project, observation, timeout):
    start = time.monotonic()
    observation.update(ready=False, polls=0, timeout_s=timeout)
    while True:
        remaining = start + timeout - time.monotonic()
        require(remaining > 0, "Initial full-text projection still pending on B")
        raw = sql(compose, project, """SELECT json_build_object(
            'ready', NOT EXISTS (SELECT FROM federation_search_jobs WHERE kind='initial'),
            'last_track_id', (SELECT last_track_id FROM federation_search_jobs WHERE kind='initial' LIMIT 1))
        """, timeout=min(10, remaining))
        observation.update(json.loads(raw), elapsed_s=round(time.monotonic() - start, 3))
        observation["polls"] += 1
        if observation["ready"]:
            return
        time.sleep(min(2, max(0, start + timeout - time.monotonic())))


def explain(compose, project, report_path):
    artifact = Path(report_path).with_name("local-search-explain.json")
    samples, plans = [], []
    # Check both selectivities and both prepared plan modes for ranked titles.
    # Sample the two skipped availability queries once to diagnose the old path.
    for term in ("Quartz0001:*", "Melody00:*"):
        lookups = [("tracks", "generic"), ("tracks", "custom"), ("name_matches", "generic")]
        if term == "Quartz0001:*":
            lookups.extend([("artists", "generic"), ("albums", "generic")])
        for category, mode in lookups:
            types, values = ("text", ":'term'") if category == "name_matches" else (
                "text,bigint,bigint,bool,bool", ":'term',10,0,true,true")
            query = (QUERIES / (category + ".sql")).read_text()
            script = f"""BEGIN READ ONLY;
                SET LOCAL statement_timeout = '8s';
                SET LOCAL lock_timeout = '1s';
                SET LOCAL plan_cache_mode = force_{mode}_plan;
                PREPARE lookup({types}) AS {query};
                EXPLAIN (ANALYZE, BUFFERS, SETTINGS, FORMAT JSON) EXECUTE lookup({values});
                ROLLBACK;"""
            sample = {"term": term, "category": category, "plan_mode": mode}
            try:
                raw = compose(project, "exec", "-T", "db-b", "psql", "-XqAt", "-U", "soundtime",
                              "-d", "soundtime", "-v", "ON_ERROR_STOP=1", "-v", "term=" + term,
                              data=script.encode(), timeout=14)
                plan, = json.loads(raw)
                sample.update(planning_ms=plan["Planning Time"], execution_ms=plan["Execution Time"],
                              rows=plan["Plan"]["Actual Rows"], jit=plan.get("JIT"))
                plans.append({**sample, "explain": plan})
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
                diagnostic = (error.stderr or b"").decode(errors="replace").strip()
                sample["error"] = diagnostic[-1024:] or str(error)
                plans.append(sample)
            samples.append(sample)
            artifact.write_text(json.dumps(plans, indent=2) + "\n")
    return {"plan_file": artifact.name, "statement_timeout_s": 8, "samples": samples}
