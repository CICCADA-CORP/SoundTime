"""Exercise the portable entry points without requiring a built release."""
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import unittest

WRAPPERS = Path(__file__).resolve().parents[1] / "packaging" / "wrappers"


@unittest.skipIf(os.name == "nt", "POSIX shell")
class PosixWrapperTests(unittest.TestCase):
    def test_dispatch_architecture_quoting_symlinks_and_exit_status(self):
        with tempfile.TemporaryDirectory(prefix="SoundTime test ") as temp:
            base = Path(temp)
            package = base / "Portable package"
            package.mkdir()
            entry = package / "soundtime"
            shutil.copy2(WRAPPERS / entry.name, entry)
            shortcut = base / "my launcher"
            shortcut.symlink_to(Path(package.name) / entry.name)
            commands = base / "commands"
            commands.mkdir()
            for name, body in (("uname", 'case "$1" in -m) echo "$TEST_CPU";; -s) echo "$TEST_KERNEL";; esac'),
                               ("sysctl", 'echo "$TEST_ROSETTA"')):
                script = commands / name
                script.write_text("#!/bin/sh\n" + body + "\n")
                script.chmod(0o700)
            for kernel, cpu, translated, target in [
                    ("Darwin", "x86_64", "0", "macos-x64"), ("Darwin", "arm64", "0", "macos-arm64"),
                    ("Darwin", "x86_64", "1", "macos-arm64"), ("Linux", "x86_64", "0", "linux-x64"),
                    ("Linux", "aarch64", "0", "linux-arm64")]:
                with self.subTest(target=target, cpu=cpu):
                    engine = package / "bin" / target / "soundtime"
                    engine.parent.mkdir(parents=True, exist_ok=True)
                    engine.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\"\nexit 37\n")
                    engine.chmod(0o600)
                    env = dict(os.environ, PATH=str(commands) + ":" + os.environ["PATH"], TEST_CPU=cpu,
                               TEST_KERNEL=kernel, TEST_ROSETTA=translated)
                    args = ["left & right", "", "l'été", "--", "a; b", "-L"]
                    result = subprocess.run(["sh", str(shortcut), *args], cwd=base,
                                            env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 37, result.stderr)
                    self.assertEqual(result.stdout.splitlines(), args)
                    result = subprocess.run(["sh", "soundtime", *args], cwd=package,
                                            env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 37)
                    self.assertEqual(result.stdout.splitlines(), args)

    def test_incomplete_archive_is_actionable(self):
        with tempfile.TemporaryDirectory() as temp:
            entry = Path(temp) / "soundtime"
            shutil.copy2(WRAPPERS / entry.name, entry)
            result = subprocess.run([str(entry)], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("extract the complete archive", result.stderr)


@unittest.skipUnless(os.name == "nt", "native Windows command launcher")
class WindowsWrapperTests(unittest.TestCase):
    def test_bitness_quotes_and_return_code(self):
        with tempfile.TemporaryDirectory(prefix="SoundTime test ! ") as temp:
            base = Path(temp)
            entry = base / "soundtime.cmd"
            shutil.copy2(WRAPPERS / entry.name, entry)
            comspec = os.environ["COMSPEC"]
            for suffix in ("x64", "arm64"):
                engine = base / "bin" / ("windows-" + suffix) / "soundtime.exe"
                engine.parent.mkdir(parents=True)
                shutil.copyfile(comspec, engine)
            # CMDCMDLINE identifies the copy of cmd.exe actually launched. Both
            # binaries are present, so a passing exit code alone is insufficient.
            (base / "probe.cmd").write_text(
                '@echo off\nsetlocal EnableExtensions DisableDelayedExpansion\n'
                'echo %CMDCMDLINE%\necho ARG=[%~1]\nexit /b %~2\n', encoding="ascii")
            # Windows can restore the real architecture when starting cmd.exe.
            # Set the simulated values inside the process that calls the wrapper.
            (base / "simulate.cmd").write_text(
                '@echo off\nset "PROCESSOR_ARCHITECTURE=%~1"\nset "PROCESSOR_ARCHITEW6432=%~2"\n'
                'call soundtime.cmd /d /c probe.cmd "music folder" 37\nexit /b %ERRORLEVEL%\n',
                encoding="ascii")
            for arch, native, expected in (("AMD64", "", "x64"), ("ARM64", "", "arm64"),
                                           ("x86", "AMD64", "x64"), ("x86", "ARM64", "arm64")):
                with self.subTest(shell="cmd", arch=arch, native=native):
                    run = subprocess.run([comspec, "/d", "/v:off", "/c", "simulate.cmd", arch, native],
                                         cwd=base, capture_output=True)
                    self.assertEqual(run.returncode, 37, run.stderr)
                    self.assertIn(f"bin\\windows-{expected}\\soundtime.exe".encode(), run.stdout.lower())
                    self.assertIn(b"ARG=[music folder]", run.stdout.splitlines())

            # Launch PowerShell normally so its child cmd.exe sees the runner's
            # actual architecture, including on native Windows ARM64.
            cpu = os.environ.get("RUNNER_ARCH") or platform.machine()
            expected = {"AMD64": "x64", "X64": "x64", "X86_64": "x64", "ARM64": "arm64"}[cpu.upper()]
            run = subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command",
                                  "& '.\\soundtime.cmd' /d /c probe.cmd 'music folder' 39; exit $LASTEXITCODE"],
                                 cwd=base, capture_output=True)
            self.assertEqual(run.returncode, 39, run.stderr)
            self.assertIn(f"bin\\windows-{expected}\\soundtime.exe".encode(), run.stdout.lower())
            self.assertIn(b"ARG=[music folder]", run.stdout.splitlines())
