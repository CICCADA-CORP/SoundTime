#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "Usage: scripts/test-p2p-federation.sh baseline|compatibility|acceptance"
    echo "Set FEDERATION_ARTIFACT_DIR to choose where each run stores its report and logs."
}

if [[ $# == 1 && $1 == --help ]]; then
    usage
    exit 0
fi
if [[ $# != 1 ]] || [[ $1 != baseline && $1 != compatibility && $1 != acceptance ]]; then
    usage >&2
    exit 2
fi

mode=$1
export FEDERATION_EXPLICIT_A=false FEDERATION_EXPLICIT_B=false FEDERATION_EXPLICIT_C=false FEDERATION_RANGES=false
export FEDERATION_CACHE_SIZE=1GB FEDERATION_VERSIONED=false
if [[ $mode != baseline ]]; then
    export FEDERATION_EXPLICIT_A=true FEDERATION_EXPLICIT_B=true
fi
if [[ $mode == acceptance ]]; then
    export FEDERATION_EXPLICIT_C=true FEDERATION_RANGES=true FEDERATION_CACHE_SIZE=0 FEDERATION_VERSIONED=true
fi
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
# Isolate generated credentials and test logs, including artifacts retained on failure.
umask 077
artifact_dir=${FEDERATION_ARTIFACT_DIR:-${TMPDIR:-/tmp}/soundtime-federation}
mkdir -p "$artifact_dir"
run_dir=$(mktemp -d "$artifact_dir/run.XXXXXXXX")
project="st-fed-$(python3 -c 'import secrets; print(secrets.token_hex(8))')"
export FEDERATION_SECRET
FEDERATION_SECRET=$(python3 -c 'import secrets; print(secrets.token_hex(32))')
compose=(docker compose --env-file /dev/null --project-name "$project" --file "$root/scripts/federation/compose.yml")
stage=prerequisites
resources_started=false
images_started=false
revision=$(git rev-parse HEAD)
dirty=false
if [[ -n $(git status --porcelain) ]]; then
    dirty=true
fi

redact_audio_tokens() {
    # The player passes its JWT in the stream URL, which the Nginx access log records.
    sed -E 's/([?&]token=)[A-Za-z0-9_.%-]+/\1[redacted]/g'
}

finish() {
    status=$?
    trap - EXIT
    set +e
    if [[ $resources_started == true ]]; then
        "${compose[@]}" ps -a > "$run_dir/containers.txt"
        "${compose[@]}" logs --no-color 2>&1 | redact_audio_tokens > "$run_dir/containers.log"
        "${compose[@]}" --profile migration down --volumes --timeout 10 > "$run_dir/cleanup.log" 2>&1 || status=1
    fi
    if [[ $images_started == true ]]; then
        for image in "soundtime-federation-backend:$project" "soundtime-federation-frontend:$project" "soundtime-federation-migration:$project"; do
            if docker image inspect "$image" > /dev/null 2>&1; then
                docker image rm --no-prune "$image" >> "$run_dir/cleanup.log" 2>&1 || status=1
            fi
        done
    fi
    python3 - "$run_dir/run.json" "$mode" "$stage" "$status" "$project" "$revision" "$dirty" <<'PY'
import json
import sys
from pathlib import Path

path, mode, stage, status, project, revision, dirty = sys.argv[1:]
with Path(path).open("w") as output:
    json.dump({"revision": revision, "dirty_worktree": dirty == "true", "project": project,
               "mode": mode, "stage": stage, "exit_code": int(status)}, output, indent=2)
    output.write("\n")
PY
    if [[ $? != 0 ]]; then
        status=1
    fi
    echo "Federation run: $run_dir (stage: $stage, exit: $status)"
    exit "$status"
}
trap finish EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

echo "Federation $mode artifacts: $run_dir"
docker info --format '{{.ServerVersion}}' > "$run_dir/docker-version.txt"
"${compose[@]}" version >> "$run_dir/docker-version.txt"
"${compose[@]}" config --quiet
stage=build
images_started=true
for service in backend-a frontend-a migration-fixture; do
    "${compose[@]}" build "$service" 2>&1 | tee "$run_dir/build-$service.log"
done

stage=startup
resources_started=true
"${compose[@]}" up --detach --no-build --wait --wait-timeout 180 proxy-a proxy-b proxy-c 2>&1 | tee "$run_dir/startup.log"

# A fourth disposable database on db-a, with no API process or storage scanner attached.
stage=historical-upgrade
"${compose[@]}" exec -T db-a createdb -U soundtime soundtime_federation_history
"${compose[@]}" run --rm -T --no-deps migration-fixture > "$run_dir/historical-references.jsonl" 2> "$run_dir/historical-upgrade.log"

proxy_url() {
    local address
    address=$("${compose[@]}" port "proxy-$1" 80)
    if [[ ! $address =~ ^127\.0\.0\.1:[0-9]+$ ]]; then
        echo "Expected a single loopback proxy for $1, got: $address" >&2
        return 1
    fi
    echo "http://$address"
}

url_a=$(proxy_url a)
url_b=$(proxy_url b)
url_c=$(proxy_url c)
stage=scenario
python3 scripts/federation/scenario.py \
    --a "$url_a" --b "$url_b" --c "$url_c" \
    --mode "$mode" --report "$run_dir/report.json" 2>&1 | redact_audio_tokens | tee "$run_dir/scenario.log"
stage=complete
