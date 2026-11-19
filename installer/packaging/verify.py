"""Run real commands from the packaged terminal launcher without Docker or a display."""
import argparse
import hashlib
import json
import secrets
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

from build import ROOT, OUT, machine, relative_binary

sys.path.insert(0, str(ROOT))
from installer import remote


def verify_agent(binary, folder, system):
    # Run the actual staging commands in the target OS shell, including the
    # PowerShell stdin byte stream used by Windows OpenSSH.
    env = {**os.environ, "HOME": str(folder), "TMP": str(folder), "TEMP": str(folder)}
    shell = ["cmd.exe", "/d", "/c"] if system == "windows" else ["/bin/sh", "-c"]
    name = ".soundtime-installer-" + secrets.token_hex(16)
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    upload, execute, cleanup = remote._staging_commands(system, name, digest)
    try:
        with binary.open("rb") as data:
            subprocess.run([*shell, upload], stdin=data, env=env, check=True, timeout=120)
        request = json.dumps({"os": system, "action": "check", "settings": {}}).encode("ascii") + b"\n"
        result = subprocess.run([*shell, execute], input=request, env=env, check=True,
                                capture_output=True, timeout=60)
        payload, = [json.loads(line[len(remote.PREFIX):]) for line in result.stdout.splitlines()
                    if line.startswith(remote.PREFIX)]
        if payload["kind"] != "result" or payload["result"]["checks"][0]["id"] != "settings":
            raise ValueError("Staged agent did not return a settings check")
    finally:
        subprocess.run([*shell, cleanup], env=env, check=True, timeout=60)
    if (folder / name).exists():
        raise ValueError("Temporary agent directory remains after cleanup")


def verify(directory):
    system, arch = machine()
    binary = directory / relative_binary(system, arch)
    if not binary.is_file():
        raise FileNotFoundError(binary)
    with tempfile.TemporaryDirectory(prefix="SoundTime CLI test ") as tmp:
        # A relocatable directory with spaces exercises shell quoting on all platforms.
        work = Path(tmp) / "SoundTime release"
        target = work / relative_binary(system, arch)
        target.parent.mkdir(parents=True)
        shutil.copy2(binary, target)
        target.chmod(0o755)
        verify_agent(target, work, system)
        for name in ("soundtime", "soundtime.cmd"):
            wrapper = ROOT / "installer" / "packaging" / "wrappers" / name
            shutil.copy2(wrapper, work / name)
        def run(args):
            env = {**os.environ, "PYTHONIOENCODING": "utf-8", "DISPLAY": ""}
            if system == "windows":
                command = ["cmd.exe", "/d", "/c", "soundtime.cmd", *args]
            else:
                command = ["sh", str(work / "soundtime"), *args]
            return subprocess.run(command, env=env, cwd=work, capture_output=True, timeout=90)

        outputs = []
        for lang in ("en", "fr", "es", "ru", "zh"):
            result = run(["--lang", lang, "--help"])
            if result.returncode or not result.stdout.strip():
                raise RuntimeError(f"Failed to open {lang} CLI help: {result.stderr!r}")
            outputs.append(result.stdout)
        if len(set(outputs)) != len(outputs):
            raise ValueError("CLI help did not change with language")
        if run(["invalid-command"]).returncode == 0:
            raise ValueError("Wrapper swallowed a command error")
        result = run(["--lang", "es", "export", "--target", "nas", "--directory", "/volume1/docker/soundtime",
                      "--output", "NAS project.zip"])
        if result.returncode:
            raise RuntimeError(f"Packaged NAS export failed: {result.stdout!r}, {result.stderr!r}")
        with zipfile.ZipFile(work / "NAS project.zip") as project:
            for name in ("compose.yaml", ".env", "qnap-compose.yaml", "docker/nginx.conf", "README.txt"):
                if not project.read(name).strip():
                    raise ValueError(f"Empty NAS project file: {name}")
        if system == "windows":
            shell = shutil.which("pwsh") or shutil.which("powershell")
            subprocess.run([shell, "-NoProfile", "-NonInteractive", "-Command",
                            r"& '.\soundtime.cmd' --lang en --help; exit $LASTEXITCODE"],
                           cwd=work, check=True, stdout=subprocess.DEVNULL, timeout=60)
    print(f"Console wrapper, five languages, exit status and NAS export verified on {system}-{arch}.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", nargs="?", type=Path, default=OUT)
    verify(parser.parse_args().directory.resolve())
