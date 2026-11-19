#!/usr/bin/env bash
set -euo pipefail
export PYTHONDONTWRITEBYTECODE=1

usage() {
    echo "Usage: scripts/test-p2p-federation.sh baseline|compatibility|historical|acceptance|shaped|relay|retention|cluster|load-smoke|load-full|rust-tests"
    echo "Set FEDERATION_BROWSER=container to run Chromium inside this disposable project."
    echo "Set FEDERATION_ARTIFACT_DIR to choose where each run stores its report and logs."
}

if [[ $# == 1 && $1 == --help ]]; then
    usage
    exit 0
fi
if [[ $# != 1 ]] || [[ $1 != baseline && $1 != compatibility && $1 != historical && $1 != acceptance && $1 != shaped && $1 != relay && $1 != retention && $1 != cluster && $1 != load-smoke && $1 != load-full && $1 != rust-tests ]]; then
    usage >&2
    exit 2
fi

mode=$1
case "$mode" in
    acceptance|retention|relay)
        export FEDERATION_RUST_LOG=${FEDERATION_RUST_LOG:-info,soundtime_server::api::audio=debug,soundtime_p2p::node::ranges=debug,soundtime_p2p::node::presence=debug}
        ;;
esac
export FEDERATION_BROWSER=${FEDERATION_BROWSER:-native}
case "$FEDERATION_BROWSER" in
    native|container) ;;
    *) echo "Choose FEDERATION_BROWSER=native or container." >&2; exit 2 ;;
esac
export FEDERATION_EXPLICIT_A=false FEDERATION_EXPLICIT_B=false FEDERATION_EXPLICIT_C=false FEDERATION_RANGES=false
export FEDERATION_CACHE_SIZE=1GB FEDERATION_VERSIONED=false FEDERATION_RETAIN_B=false
if [[ $mode != baseline ]]; then
    export FEDERATION_EXPLICIT_A=true FEDERATION_EXPLICIT_B=true
fi
if [[ $mode == acceptance || $mode == shaped || $mode == relay || $mode == retention || $mode == cluster || $mode == load-* ]]; then
    export FEDERATION_EXPLICIT_C=true FEDERATION_RANGES=true FEDERATION_CACHE_SIZE=0 FEDERATION_VERSIONED=true
fi
if [[ $mode == retention ]]; then
    export FEDERATION_CACHE_SIZE=256MB FEDERATION_RETAIN_B=true
fi
if [[ $mode == load-* ]]; then
    # Check local generator capacity before building or starting the disposable project.
    python3 "$(dirname "$0")/federation/load.py" --preflight "$mode"
    if [[ ${SOUNDTIME_LOAD_CACHE:-cold} == warm ]]; then
        export FEDERATION_CACHE_SIZE=1GB FEDERATION_RETAIN_B=true
    fi
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
if [[ $FEDERATION_BROWSER == container ]]; then
    compose+=(--file "$root/scripts/federation/browser.yml")
fi
scale=()
export SOUNDTIME_TEST_PUBLIC_LOAD=0
if [[ $mode == cluster ]]; then
    compose+=(--file "$root/scripts/federation/cluster.yml")
    scale+=(--scale catalog-http=2)
fi
if [[ $mode == load-full ]]; then
    compose+=(--file "$root/scripts/federation/load-public.yml")
    scale+=(--scale catalog-http=2)
    export SOUNDTIME_TEST_PUBLIC_LOAD=1
fi
if [[ $mode == relay ]]; then
    compose+=(--file "$root/scripts/federation/relay.yml")
    export SOUNDTIME_TEST_RELAY=1
elif [[ $mode == shaped ]]; then
    compose+=(--file "$root/scripts/federation/shaped.yml")
elif [[ $mode == historical ]]; then
    compose+=(--file "$root/scripts/federation/historical.yml")
elif [[ $mode == retention ]]; then
    compose+=(--file "$root/scripts/federation/retention.yml")
    export SOUNDTIME_TEST_RETENTION=1
elif [[ $mode == rust-tests ]]; then
    compose+=(--file "$root/scripts/federation/rust-tests.yml")
fi
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
        for image in "soundtime-federation-backend:$project" "soundtime-federation-frontend:$project" "soundtime-federation-migration:$project" "soundtime-federation-relay:$project" "soundtime-federation-link-tools:$project" "soundtime-federation-browser:$project" "soundtime-federation-rust-tests:$project" "soundtime-federation-historical:$project"; do
            if docker image inspect "$image" > /dev/null 2>&1; then
                docker image rm --no-prune "$image" >> "$run_dir/cleanup.log" 2>&1 || status=1
            fi
        done
    fi
    python3 -c '
import json
import sys
from pathlib import Path

path, mode, stage, status, project, revision, dirty = sys.argv[1:]
with Path(path).open("w") as output:
    json.dump({"revision": revision, "dirty_worktree": dirty == "true", "project": project,
               "mode": mode, "stage": stage, "exit_code": int(status)}, output, indent=2)
    output.write("\n")
' "$run_dir/run.json" "$mode" "$stage" "$status" "$project" "$revision" "$dirty"
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
if [[ $mode == historical ]]; then
    python3 scripts/federation/historical_image.py "$project" "$run_dir" 2>&1 | tee "$run_dir/build-historical-c.log"
fi
build_services=(backend-a frontend-a migration-fixture)
if [[ $mode == rust-tests ]]; then build_services=(migration-fixture rust-tests); fi
if [[ $mode == relay ]]; then build_services+=(relay); fi
if [[ $mode == shaped ]]; then build_services+=(link-tools); fi
if [[ $FEDERATION_BROWSER == container ]]; then
    case "$mode" in
        baseline|acceptance|relay|load-*|shaped) build_services+=(browser) ;;
    esac
fi
for service in "${build_services[@]}"; do
    "${compose[@]}" build "$service" 2>&1 | tee "$run_dir/build-$service.log"
done

stage=startup
resources_started=true
export SOUNDTIME_TEST_COMPOSE_PROJECT="$project"
if [[ $mode == load-full ]]; then
    stage=public-index
    "${compose[@]}" up --detach --no-build --wait --wait-timeout 180 --scale catalog-http=2 catalog-http
    export SOUNDTIME_TEST_LOAD_INDEX="$run_dir/load-index.json"
    python3 scripts/federation/load.py --prepare-public "$SOUNDTIME_TEST_LOAD_INDEX"
    stage=startup
fi
services=(proxy-a proxy-b proxy-c)
if [[ $mode == rust-tests ]]; then services=(db-a); fi
"${compose[@]}" up --detach --no-build --wait --wait-timeout 180 ${scale[@]+"${scale[@]}"} "${services[@]}" 2>&1 | tee "$run_dir/startup.log"

# A fourth disposable database on db-a, with no API process or storage scanner attached.
stage=historical-upgrade
"${compose[@]}" exec -T db-a createdb -U soundtime soundtime_federation_history
"${compose[@]}" run --rm -T --no-deps migration-fixture > "$run_dir/historical-references.jsonl" 2> "$run_dir/historical-upgrade.log"

if [[ $mode == rust-tests ]]; then
    stage=rust-and-postgres
    "${compose[@]}" exec -T db-a createdb -U soundtime --template soundtime_federation_history soundtime_federation_rust
    tests=(cargo test --offline --locked --release --workspace --features soundtime-server/redis --lib --bins --tests)
    "${compose[@]}" run --rm -T --no-deps rust-tests "${tests[@]}" -- --test-threads=1 2>&1 | tee "$run_dir/rust-tests.log"
    "${compose[@]}" run --rm -T --no-deps rust-tests "${tests[@]}" -- --ignored --test-threads=1 2>&1 | tee "$run_dir/postgres-tests.log"
    stage=complete
    exit 0
fi
if [[ $mode == relay ]]; then
    stage=relay-isolation
    ids=()
    for service in backend-a backend-b backend-c relay; do
        ids+=("$("${compose[@]}" ps -q "$service")")
    done
    python3 scripts/federation/relay-topology.py "${ids[@]}" > "$run_dir/relay-topology.json"
fi

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
scenario=scripts/federation/scenario.py
if [[ $mode == historical ]]; then
    scenario=scripts/federation/historical.py
    SOUNDTIME_HISTORICAL_C=$("${compose[@]}" ps -q backend-c)
    export SOUNDTIME_HISTORICAL_C
fi
if [[ $mode == shaped ]]; then scenario=scripts/federation/shaped.py; fi
if [[ $mode == retention ]]; then scenario=scripts/federation/retention.py; fi
if [[ $mode == load-* ]]; then scenario=scripts/federation/load.py; fi
if [[ $mode == cluster ]]; then scenario=scripts/federation/cluster.py; fi
python3 "$scenario" \
    --a "$url_a" --b "$url_b" --c "$url_c" \
    --mode "$mode" --report "$run_dir/report.json" 2>&1 | redact_audio_tokens | tee "$run_dir/scenario.log"
stage=complete
