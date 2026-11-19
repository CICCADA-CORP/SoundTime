"""Run the private HTTP generator and collect progress even when its control pipe fails."""
from contextlib import contextmanager
import json
import os
from pathlib import Path
from queue import Empty, Queue
import re
import signal
import subprocess
import tempfile
import threading
import time

from scenario import ScenarioError, require


HERE = Path(__file__).resolve().parent


def location():
    selected = os.getenv("SOUNDTIME_LOAD_GENERATOR") or os.getenv("FEDERATION_BROWSER", "native")
    require(selected in ("native", "container"), "Choose SOUNDTIME_LOAD_GENERATOR=native or container")
    return selected


def commands(project, selected):
    require(re.fullmatch(r"st-fed-[a-f0-9]{16}", project), "Expected a disposable Compose project")
    if selected == "native":
        return ["node", str(HERE / "load-client.mjs")], None
    require(selected == "container", "Unknown generator location")
    compose = ["docker", "compose", "--env-file", "/dev/null", "--project-name", project,
               "-f", str(HERE / "compose.yml"), "-f", str(HERE / "browser.yml")]
    return (compose + ["exec", "-T", "load-generator", "node", "/opt/soundtime-browser/load-client.mjs"],
            compose + ["stop", "--timeout", "3", "load-generator"])


class Records:
    def __init__(self, report):
        self.report = report
        self.pending = Queue()
        report["generator_samples"] = []

    def drain(self, stream, progress):
        try:
            while True:
                line = stream.readline(8 * 1024 * 1024 + 1)
                if not line:
                    break
                require(line.endswith(b"\n") and len(line) <= 8 * 1024 * 1024, "Invalid generator frame size")
                row = json.loads(line)
                phase = row["phase"]
                if phase in ("started", "steady", "sample"):
                    progress.write(line)
                    progress.flush()
                    if phase == "sample":
                        self.report["generator_samples"].append(row)
                        continue
                    self.report["generator_" + phase] = row
                    if phase == "started":
                        continue
                elif phase == "complete":
                    self.report["generator_result"] = row
                else:
                    raise ScenarioError("Unknown generator frame")
                self.pending.put((phase, row, time.monotonic()))
        except Exception as error:
            self.pending.put(("error", str(error), 0))
        finally:
            self.pending.put(("eof", None, 0))

    def wait(self, expected, timeout):
        try:
            phase, row, received = self.pending.get(timeout=max(0, timeout))
        except Empty as error:
            raise ScenarioError(f"Generator timed out before {expected}") from error
        require(phase == expected, f"Generator ended before {expected} ({phase})")
        return row, received


@contextmanager
def launch(project, selected, cfg, report, progress_path):
    cmd, stop = commands(project, selected)
    cfg = dict(cfg, base="http://proxy-b" if selected == "container" else cfg["base"])
    env = os.environ.copy()
    env.pop("SOUNDTIME_LOAD_INGRESS", None)  # set exclusively by the Compose service
    records = Records(report)
    with progress_path.open("wb") as progress, tempfile.TemporaryFile() as stderr, subprocess.Popen(
        cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, env=env, start_new_session=True
    ) as proc:
        def write_config():
            try:
                proc.stdin.write((json.dumps(cfg) + "\n").encode())
                proc.stdin.close()
            except (BrokenPipeError, OSError, ValueError):
                pass

        reader = threading.Thread(target=records.drain, args=(proc.stdout, progress), daemon=True)
        reader.start()
        writer = threading.Thread(target=write_config, daemon=True)
        writer.start()

        def interrupt():
            try:
                os.killpg(proc.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            if stop:
                try:
                    subprocess.run(stop, check=True, capture_output=True, timeout=12)
                except (OSError, subprocess.SubprocessError) as error:
                    report["generator_stop_error"] = str(error)
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(proc.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

        stopped = False
        def on_signal(signum, _frame):
            nonlocal stopped
            stopped = True
            interrupt()
            raise SystemExit(128 + signum)

        previous = {s: signal.signal(s, on_signal) for s in (signal.SIGTERM, signal.SIGINT)}
        try:
            yield proc, records
        finally:
            for s, handler in previous.items():
                signal.signal(s, handler)
            if not stopped and (proc.poll() is None or proc.returncode != 0):
                interrupt()
            writer.join(2)
            reader.join(2)
            report["generator_exit_code"] = proc.poll()
            stderr.seek(max(0, stderr.tell()-4096))
            tail = stderr.read().decode(errors="replace").strip()
            if tail:
                for token in cfg["tokens"]:
                    tail = tail.replace(token, "[redacted]")
                report["generator_stderr_tail"] = tail
