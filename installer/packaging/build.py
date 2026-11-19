"""Build one native console executable, or assemble verified builds into one portable CLI."""
import argparse
import hashlib
import os
import platform
import shutil
import stat
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "dist" / "installer"
ARCHIVE = "SoundTime-CLI.zip"
SYSTEMS = ("linux", "windows", "macos")
ARCHES = ("x64", "arm64")


def machine():
    system = {"Darwin": "macos", "Linux": "linux", "Windows": "windows"}[platform.system()]
    arch = {"arm64": "arm64", "aarch64": "arm64", "x86_64": "x64", "amd64": "x64"}[platform.machine().lower()]
    return system, arch


def relative_binary(system, arch):
    return Path("bin", f"{system}-{arch}", "soundtime.exe" if system == "windows" else "soundtime")


def build():
    system, arch = machine()
    output = OUT / relative_binary(system, arch)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="soundtime-console-") as tmp:
        work = Path(tmp)
        command = [
            sys.executable, "-m", "PyInstaller", "--onefile", "--console", "--noupx",
            "--noconfirm", "--log-level", "WARN", "--name", "soundtime",
            "--paths", str(ROOT), "--specpath", str(work), "--workpath", str(work / "build"),
            "--distpath", str(work / "dist"), "--exclude-module", "tkinter",
            "--add-data", f"{ROOT / 'docker-compose.yml'}:.",
            "--add-data", f"{ROOT / 'docker' / 'nginx.conf'}:docker",
            str(ROOT / "installer" / "launcher.py"),
        ]
        subprocess.run(command, cwd=ROOT, check=True, env={**os.environ, "PYINSTALLER_CONFIG_DIR": str(work / "cache")})
        shutil.copy2(work / "dist" / output.name, output)
    print(output)


def assemble(directory):
    wrappers = ROOT / "installer" / "packaging" / "wrappers"
    files = [(wrappers / name, name) for name in ("soundtime", "soundtime.cmd")]
    files.append((ROOT / "installer" / "packaging" / "LIRE-MOI.txt", "README.txt"))
    for system in SYSTEMS:
        for arch in ARCHES:
            rel = relative_binary(system, arch)
            files.append((directory / rel, rel.as_posix()))
    OUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=OUT) as tmp:
        path = Path(tmp) / ARCHIVE
        with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
            for src, name in files:
                if not src.is_file() or not src.stat().st_size:
                    raise ValueError(f"Missing release file: {src}")
                info = zipfile.ZipInfo.from_file(src, name)
                info.create_system = 3
                # The Actions artifact service normalizes file modes on download.
                mode = 0o644 if name.endswith((".cmd", ".txt")) else 0o755
                info.external_attr = (stat.S_IFREG | mode) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                with src.open("rb") as source, archive.open(info, "w") as target:
                    shutil.copyfileobj(source, target)
        path.replace(OUT / ARCHIVE)
    checksum = hashlib.sha256((OUT / ARCHIVE).read_bytes()).hexdigest()
    (OUT / (ARCHIVE + ".sha256")).write_bytes(f"{checksum}  {ARCHIVE}\n".encode("ascii"))
    print(OUT / ARCHIVE)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assemble", type=Path, metavar="DIRECTORY", help="Folder containing bin/<os>-<arch> binaries")
    args = parser.parse_args()
    if args.assemble:
        assemble(args.assemble)
    else:
        build()
