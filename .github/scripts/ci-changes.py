"""Select checks and native image builds for pull requests and main releases."""

import json
import os
import subprocess
from pathlib import Path


components = ("backend", "frontend")
platforms = {"amd64": "ubuntu-24.04", "arm64": "ubuntu-24.04-arm"}
release = os.environ["GITHUB_EVENT_NAME"] == "push"
paths = set()
if not release:
    # Actions checks out the PR merge commit. Its first parent is the tested base.
    diff = subprocess.check_output(["git", "diff", "--name-only", "--no-renames", "-z", "HEAD^1", "HEAD"])
    paths = {os.fsdecode(path) for path in diff.split(b"\0") if path}

ci_files = {".github/workflows/ci.yml", ".github/workflows/container-build.yml", ".github/scripts/ci-changes.py"}
shared = release or bool(paths & ci_files) or any(path.startswith(".github/actions/") for path in paths)
matrix = []
with Path(os.environ["GITHUB_OUTPUT"]).open("a") as outputs:
    for component in components:
        check = shared or any(path.startswith(f"{component}/") for path in paths)
        print(f"{component}={str(check).lower()}", file=outputs)
        if check or f"docker/Dockerfile.{component}" in paths:
            for arch, runner in platforms.items():
                matrix.append({"component": component, "arch": arch, "runner": runner})
    config = shared or "install.sh" in paths or any(
        path == "docker-compose.yml" or path.startswith("docker/compose.") for path in paths
    )
    private_listening = shared or bool(paths & {"docker/nginx.conf", "scripts/test-private-listening.sh"}) or any(
        path.startswith(("backend/", "frontend/", "docker/Dockerfile.", "scripts/private-listening/"))
        for path in paths
    )
    print(f"private-listening={str(private_listening).lower()}", file=outputs)
    print(f"config={str(config).lower()}", file=outputs)
    print(f"images={str(bool(matrix)).lower()}", file=outputs)
    print("matrix=" + json.dumps({"include": matrix}, separators=(",", ":")), file=outputs)
