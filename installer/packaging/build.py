"""Produce the native SoundTime installer archive for the current OS and CPU."""

import argparse
import hashlib
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "dist" / "installer"
MAC_APP = "SoundTime Installer"
PROGRAM = "SoundTime-Installer"


def checksum(archive):
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(
        f"{digest}  {archive.name}\n", encoding="ascii"
    )
    print(archive)


def build_sources():
    files = [
        ROOT / relative for relative in (
            "installer/Lancer.cmd", "installer/Lancer.command",
            "docker-compose.yml", "docker/nginx.conf", "LICENSE",
        )
    ]
    files.extend((ROOT / "installer").glob("*.py"))
    files.extend(file for file in (ROOT / "installer" / "ui").rglob("*") if file.is_file())
    OUT.mkdir(parents=True, exist_ok=True)
    archive = OUT / "SoundTime-Installer-sources.zip"
    with tempfile.TemporaryDirectory(dir=OUT) as tmp:
        packed = Path(tmp) / archive.name
        with zipfile.ZipFile(packed, "w", zipfile.ZIP_DEFLATED) as source:
            for file in sorted(files):
                source.write(file, file.relative_to(ROOT))
            source.write(ROOT / "installer" / "packaging" / "LIRE-MOI.txt", "LIRE-MOI.txt")
        packed.replace(archive)
    checksum(archive)


def build():
    system = {"Darwin": "macOS", "Linux": "Linux", "Windows": "Windows"}[platform.system()]
    arch = {"arm64": "arm64", "aarch64": "arm64", "amd64": "x64", "x86_64": "x64"}[
        platform.machine().lower()
    ]
    name = MAC_APP if system == "macOS" else PROGRAM
    extension = ".tar.gz" if system == "Linux" else ".zip"
    archive = OUT / f"SoundTime-Installer-{system}-{arch}{extension}"
    OUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="soundtime-build-") as tmp:
        work = Path(tmp)
        dist = work / "dist"
        command = [
            sys.executable, "-m", "PyInstaller", "--noconfirm", "--noupx",
            "--name", name, "--specpath", str(work), "--workpath", str(work / "build"),
            "--distpath", str(dist), "--log-level", "WARN",
            "--add-data", f"{ROOT / 'installer' / 'ui'}:installer/ui",
            "--add-data", f"{ROOT / 'docker' / 'nginx.conf'}:docker",
            "--add-data", f"{ROOT / 'docker-compose.yml'}:.",
        ]
        # The SSH launcher builds a small Python agent for the selected host.
        # Frozen Python modules alone do not expose their source for this copy.
        for module in sorted((ROOT / "installer").glob("*.py")):
            command += ["--add-data", f"{module}:installer"]
        if system in ("Windows", "macOS"):
            command += ["--icon", str(ROOT / "docs" / "assets" / "soundtime-logo.png")]
        if system == "macOS":
            command += ["--onedir", "--windowed", "--osx-bundle-identifier", "org.soundtime.installer"]
        else:
            command += ["--onefile"]
            if system == "Windows":
                command += ["--hide-console", "minimize-late"]
        command.append(str(ROOT / "installer" / "launcher.py"))
        # Keep PyInstaller caches per build, including on CI and read-only homes.
        subprocess.run(command, check=True, cwd=ROOT,
                       env={**os.environ, "PYINSTALLER_CONFIG_DIR": str(work / "cache")})

        packed = work / archive.name
        if system == "macOS":
            subprocess.run(["ditto", "-c", "-k", "--norsrc", "--keepParent",
                            str(dist / f"{name}.app"), str(packed)], check=True)
        elif system == "Windows":
            with zipfile.ZipFile(packed, "w", zipfile.ZIP_DEFLATED) as out:
                out.write(dist / f"{name}.exe", f"{name}.exe")
        else:
            with tarfile.open(packed, "w:gz") as out:
                out.add(dist / name, arcname=name)
        # A failed build leaves the previous package intact. Stage the rename on
        # the destination filesystem in case the temp directory is another disk.
        with tempfile.NamedTemporaryFile(dir=OUT, delete=False) as dest:
            with packed.open("rb") as source:
                shutil.copyfileobj(source, dest)
        Path(dest.name).replace(archive)
    checksum(archive)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sources", action="store_true", help="package the portable Python launcher")
    if parser.parse_args().sources:
        build_sources()
    else:
        build()
