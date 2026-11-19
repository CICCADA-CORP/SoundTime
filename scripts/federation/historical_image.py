"""Build the archived server and identify its running private test container."""
from ipaddress import IPv4Address
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile


REVISION = "eb9bd34543cd18dccd702ded2d2a910548e784e4"
BUILD_MARKER = "legacy-" + REVISION[:12]


def validate_project(project):
    if not re.fullmatch(r"st-fed-[a-f0-9]{16}", project):
        raise ValueError("A disposable federation project is required")


def build(project, workspace):
    validate_project(project)
    with tempfile.TemporaryDirectory(prefix="archived-backend-", dir=workspace) as tmp:
        path = Path(tmp).resolve()
        archive = path / "source.tar"
        # Read only the requested tree, even when the active checkout is dirty.
        subprocess.run(["git", "archive", "--format=tar", "--output", str(archive),
                        REVISION, "backend", "docker/Dockerfile.backend"], check=True)
        subprocess.run(["tar", "-xf", str(archive), "-C", tmp], check=True)
        subprocess.run(["docker", "build", "--tag", "soundtime-federation-historical:" + project,
                        "--build-arg", "SOUNDTIME_BUILD_NUMBER=" + BUILD_MARKER,
                        "--label", "org.opencontainers.image.revision=" + REVISION,
                        "--file", str(path / "docker/Dockerfile.backend"), str(path / "backend")], check=True)


def inspect_c(project, container):
    validate_project(project)
    if not re.fullmatch(r"[a-f0-9]{12,64}", container):
        raise ValueError("Missing running historical backend")

    def field(template):
        return subprocess.check_output(["docker", "container", "inspect", "--format", template, container],
                                       text=True).strip()

    expected = {"com.docker.compose.project": project, "com.docker.compose.service": "backend-c",
                "org.opencontainers.image.revision": REVISION}
    for key, value in expected.items():
        if field('{{index .Config.Labels "' + key + '"}}') != value:
            raise ValueError("Unexpected historical container label: " + key)
    networks = json.loads(field("{{json .NetworkSettings.Networks}}"))
    if set(networks) != {project + "_peers", project + "_site-c"}:
        raise ValueError("The archived binary has an unexpected network route")
    internal = subprocess.check_output(["docker", "network", "inspect", "--format", "{{.Internal}}",
                                        *sorted(networks)], text=True).splitlines()
    if internal != ["true", "true"] or json.loads(field("{{json .HostConfig.Dns}}")) != ["127.0.0.1"]:
        raise ValueError("The archived binary needs private networks and local-only DNS")
    ip = IPv4Address(networks[project + "_peers"]["IPAddress"])
    if ip.is_multicast or ip.is_loopback or ip.is_unspecified:
        raise ValueError("Unexpected address on the private peers network")
    return {"revision": REVISION, "image_id": field("{{.Image}}"), "networks": sorted(networks),
            "direct_address": str(ip) + ":11204"}


if __name__ == "__main__":
    build(*sys.argv[1:])
