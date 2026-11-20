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
        def run(args, *, input_data=None):
            env = {**os.environ, "PYTHONIOENCODING": "utf-8", "DISPLAY": "",
                   "SOUNDTIME_LANG": "fr", "LANGUAGE": "fr", "LC_MESSAGES": "fr_FR.UTF-8"}
            if system == "windows":
                command = ["cmd.exe", "/d", "/c", "soundtime.cmd", *args]
            else:
                command = ["sh", str(work / "soundtime"), *args]
            return subprocess.run(command, env=env, cwd=work, input=input_data, capture_output=True, timeout=90)

        help_text = run(["--help"])
        if help_text.returncode or b"Install SoundTime" not in help_text.stdout:
            raise ValueError("The packaged CLI must show English help on a French system")
        legacy = run(["--lang", "fr", "--help"])
        if legacy.returncode or legacy.stdout != help_text.stdout:
            raise ValueError("The legacy locale option changed the help language")
        wizard = run([], input_data=b"2\n2\n1\n1\nCancelled project.zip\n2\n")
        if (wizard.returncode or b"Where do you want to install SoundTime?" not in wizard.stdout
                or b"Apply these settings?" not in wizard.stdout or (work / "Cancelled project.zip").exists()):
            raise ValueError("The packaged wizard must offer the English NAS flow and allow cancellation")
        if run(["invalid-command"]).returncode == 0:
            raise ValueError("Wrapper swallowed a command error")
        result = run(["export", "--target", "nas", "--access", "lan", "--directory", "/volume1/docker/soundtime",
                      "--output", "NAS project.zip"])
        if result.returncode:
            raise RuntimeError(f"Packaged NAS export failed: {result.stdout!r}, {result.stderr!r}")
        with zipfile.ZipFile(work / "NAS project.zip") as project:
            for name in ("compose.yaml", ".env", "qnap-compose.yaml", "docker/nginx.conf", "README.txt"):
                if not project.read(name).strip():
                    raise ValueError(f"Empty NAS project file: {name}")
            if not project.read("docker/access/users").startswith(b"soundtime:$2b$12$"):
                raise ValueError("The packaged NAS LAN export must include a bcrypt bootstrap credential")
            if b"Install SoundTime on your NAS" not in project.read("README.txt"):
                raise ValueError("The exported NAS guide must be in English")
        if system == "windows":
            shell = shutil.which("pwsh") or shutil.which("powershell")
            subprocess.run([shell, "-NoProfile", "-NonInteractive", "-Command",
                            r"& '.\soundtime.cmd' --help; exit $LASTEXITCODE"],
                           cwd=work, check=True, stdout=subprocess.DEVNULL, timeout=60)
    print(f"Console wrapper, English wizard and help, exit status and English NAS export verified on {system}-{arch}.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", nargs="?", type=Path, default=OUT)
    verify(parser.parse_args().directory.resolve())
