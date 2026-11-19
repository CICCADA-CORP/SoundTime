"""Capture the media worker's exact provider query without opening a stream."""
import json
from pathlib import Path
from uuid import UUID


def explain(compose, project, track, report_path):
    query = (Path(__file__).resolve().parents[2] /
             "backend/crates/soundtime-p2p/src/range_transport/providers.sql").read_text()
    # SeaORM/SQLx binds these as prepared parameters. Measure the generic plan
    # reused by warm worker connections across many tracks and suppliers.
    script = r"""
BEGIN READ ONLY;
SET LOCAL statement_timeout = '15s';
SET LOCAL plan_cache_mode = force_generic_plan;
SELECT c.node_id AS issuer,p.origin_publication_id AS publication,p.revision AS revision,
       v.content_hash AS hash,v.file_size AS size
  FROM track_publications p JOIN federation_catalogs c ON c.id=p.catalog_id
  JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision
 WHERE p.track_id=:'track' AND p.state='active'
\gset
PREPARE candidates(uuid,text,uuid,bigint,text,bigint,boolean,bigint,text) AS
""" + query + r""";
EXPLAIN (ANALYZE,BUFFERS,FORMAT JSON)
    EXECUTE candidates(:'track',:'issuer',:'publication',:revision,:'hash',:size,true,12,NULL);
ROLLBACK;
"""
    raw = compose(project, "exec", "-T", "db-b", "psql", "-XqAt", "-U", "soundtime", "-d", "soundtime",
                  "-v", "ON_ERROR_STOP=1", "-v", "track="+str(UUID(track)), data=script.encode(), timeout=25)
    plan, = json.loads(raw)
    artifact = Path(report_path).with_name("range-providers-explain.json")
    artifact.write_text(json.dumps(plan, indent=2) + "\n")
    root = plan["Plan"]
    if root["Actual Rows"] < 1:
        raise ValueError("The measured audio publication has no eligible provider")
    return {"track_id": track, "plan": artifact.name, "providers": root["Actual Rows"],
            "execution_ms": plan["Execution Time"], "planning_ms": plan["Planning Time"],
            "shared_hit_blocks": root["Shared Hit Blocks"], "shared_read_blocks": root["Shared Read Blocks"]}
