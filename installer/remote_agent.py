"""Execute installer operations sent by the user's authenticated OpenSSH client."""
import json
import os
import platform
import sys
from pathlib import Path

if __package__:
    from . import engine
else:
    import engine

PREFIX = "SOUNDTIME-INSTALLER "


def emit(kind, **fields):
    print(PREFIX + json.dumps({"kind": kind, **fields}), flush=True)


def main():
    try:
        request = json.loads(sys.stdin.buffer.readline())
        actual = {"Linux": "linux", "Darwin": "macos", "Windows": "windows"}.get(platform.system())
        if not actual or request.get("os", actual) != actual:
            raise ValueError("The detected operating system does not match the selected destination.")
        if actual == "macos":
            os.environ["PATH"] = os.pathsep.join([os.environ.get("PATH", ""), "/usr/local/bin",
                                                  "/opt/homebrew/bin", str(Path.home() / ".docker/bin"),
                                                  "/Applications/Docker.app/Contents/Resources/bin"])
        action = request["action"]
        if action == "recommend":
            recommended = engine.recommend(request.get("settings", {}).get("directory"))
            # Recommendations cross the SSH boundary. Whitelist the plan fields
            # and retain only non-secret hints for a previously configured store.
            result = {key: recommended[key] for key in ("directory", "musicDirectory", "port", "peerPort",
                                                         "domain", "access", "p2p")}
            result["storage"] = {key: value for key, value in recommended["storage"].items()
                                 if key in ("kind", "reuseCredentials", "endpoint", "region", "bucket", "prefix")}
        elif action == "check":
            result = engine.preflight(request["settings"])
        elif action in ("install", "lan"):
            operation = engine.install if action == "install" else engine.enable_lan
            result = operation(request["settings"], lambda stage, message: emit("event", stage=stage, message=message))
        else:
            raise ValueError("Unknown remote action.")
        emit("result", result=result)
        return 0
    except (ValueError, RuntimeError) as exc:
        emit("error", message=str(exc))
    except OSError:
        emit("error", message="The server could not access the folder or Docker. Check the SSH account permissions.")
    except Exception:
        emit("error", message="The remote agent encountered an error. Run the checks again.")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
