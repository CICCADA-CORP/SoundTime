#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
run_dir=$(mktemp -d "${TMPDIR:-/tmp}/soundtime-private.XXXXXXXX")
project="soundtime-private-$(openssl rand -hex 6)"
QA_SECRET=$(openssl rand -hex 32)
export QA_SECRET
export QA_BACKEND_IMAGE=${QA_BACKEND_IMAGE:-soundtime-qa/backend:$project}
export QA_FRONTEND_IMAGE=${QA_FRONTEND_IMAGE:-soundtime-qa/frontend:$project}
compose=(docker compose --project-name "$project" --env-file /dev/null -f "$root/scripts/private-listening/compose.yml")
cleanup() {
  "${compose[@]}" down --volumes --remove-orphans >/dev/null || true
  rm -f -- "$run_dir/fixture.json"
  rmdir "$run_dir" || true
}
trap cleanup EXIT
if [[ ${QA_USE_BUILT_IMAGES:-0} == 1 ]]; then
  "${compose[@]}" up -d --no-build
else
  "${compose[@]}" up -d --build
fi
port=$("${compose[@]}" port nginx 80)
export CYPRESS_BASE_URL="http://$port"
export SOUNDTIME_E2E_FIXTURE="$run_dir/fixture.json"
ready() {
  curl -fsS "$CYPRESS_BASE_URL/api/setup/status" >/dev/null 2>&1 &&
    curl -fsS "$CYPRESS_BASE_URL/login" >/dev/null 2>&1
}
for ((i=0; i<90; i++)); do
  if ready; then break; fi
  sleep 2
done
ready || { echo "The private listening instance did not become ready." >&2; exit 1; }
verify_image() {
  local service=$1 image=$2 expected running
  expected=$(docker image inspect --format '{{.Id}}' "$image")
  running=$(docker inspect --format '{{.Image}}' "$("${compose[@]}" ps -q "$service")")
  if [[ "$running" != "$expected" ]]; then
    echo "$service is running $running, expected $expected ($image)" >&2
    return 1
  fi
  echo "Verified running $service image $image ($running)"
}
verify_image backend "$QA_BACKEND_IMAGE"
verify_image frontend "$QA_FRONTEND_IMAGE"
node "$root/scripts/private-listening/seed.mjs" "$CYPRESS_BASE_URL" "$SOUNDTIME_E2E_FIXTURE"
cd "$root/frontend"
npm run cy:run -- "$@"

if [[ -n ${QA_ROLLBACK_FRONTEND_IMAGE:-} ]]; then
  candidate=$QA_FRONTEND_IMAGE
  for release in "$QA_ROLLBACK_FRONTEND_IMAGE" "$candidate"; do
    export QA_FRONTEND_IMAGE="$release"
    "${compose[@]}" up -d --no-build --no-deps --force-recreate frontend
    "${compose[@]}" exec -T nginx nginx -s reload
    for ((i=0; i<30; i++)); do
      if ready; then break; fi
      sleep 1
    done
    ready
    verify_image frontend "$release"
    npm run cy:run -- --spec cypress/private/listening.cy.ts
  done
fi
