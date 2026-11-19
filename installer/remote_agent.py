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
            raise ValueError("Le système détecté ne correspond pas à la destination choisie.")
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
            raise ValueError("Action distante inconnue.")
        emit("result", result=result)
        return 0
    except (ValueError, RuntimeError) as exc:
        emit("error", message=str(exc))
    except OSError:
        emit("error", message="Le serveur n’a pas pu accéder au dossier ou à Docker. Vérifiez les droits du compte SSH.")
    except Exception:
        emit("error", message="L’agent distant a rencontré une erreur. Relancez la vérification.")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
