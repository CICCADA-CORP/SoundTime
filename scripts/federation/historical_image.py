"""Build the archived server and identify its running private test container."""
from ipaddress import IPv4Address
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


REVISION = "eb9bd34543cd18dccd702ded2d2a910548e784e4"
BUILD_MARKER = "legacy-" + REVISION[:12]


# The public CI run 44553597395 on this commit produced build 88. The pinned
# manifests below are the two Linux entries in the published release index.
REGISTRY = "ghcr.io/ciccada-corp/soundtime-backend"
RELEASE_INDEX = "sha256:f2b4f1fc81cdb61b4d25ff88171428ba89198d3cf809655595220f2e832e9acf"
RELEASE_PLATFORMS = {
    "amd64": ("sha256:15f791d68f69e828cff327068d3b32721af51a0ca94ddccf6cd01b0c19f3a9d1",
              "sha256:f2fe7fe56f45addd76a529d268277864d1d4492a6d6d3297b53ec2a9cf0e67f5"),
    "arm64": ("sha256:f4314976e864522e0eb1d0a1afc4fef0f855d356102529764824912263f3fc1f",
              "sha256:1b2eb59b126c6498339da3191ee0c3c740ece38b2451dc2ecf9cfc1aa55d8b52"),
}


def release_platform():
    source = os.environ.get("FEDERATION_HISTORICAL_SOURCE", "archive")
    if source == "archive":
        return None
    if source != "release":
        raise ValueError("Choose archive or release as the historical image source")
    arch = subprocess.check_output(["docker", "version", "--format", "{{.Server.Arch}}"], text=True).strip()
    if arch not in RELEASE_PLATFORMS:
        raise ValueError(f"The historical release did not publish Linux/{arch}")
    return RELEASE_PLATFORMS[arch]


def validate_project(project):
    if not re.fullmatch(r"st-fed-[a-f0-9]{16}", project):
        raise ValueError("A disposable federation project is required")


def build(project, workspace):
    validate_project(project)
    platform = release_platform()
    if platform:
        image = REGISTRY + "@" + platform[0]
        subprocess.run(["docker", "pull", image], check=True)
        subprocess.run(["docker", "image", "tag", image, "soundtime-federation-historical:" + project], check=True)
        return
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
    image_id = field("{{.Image}}")
    platform = release_platform()
    if platform and image_id not in platform:
        raise ValueError("The running archived binary differs from the pinned public release")
    return {"revision": REVISION, "image_id": image_id, "networks": sorted(networks),
            "source": "release" if platform else "archive",
            "release_index": RELEASE_INDEX if platform else None,
            "pong_version": "0.1.88" if platform else "0.1." + BUILD_MARKER,
            "direct_address": str(ip) + ":11204"}


if __name__ == "__main__":
    build(*sys.argv[1:])
