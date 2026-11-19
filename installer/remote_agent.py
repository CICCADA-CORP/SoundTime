"""Runs inside a temporary directory on the selected Linux host, over OpenSSH."""
import json
import platform
import sys

import engine

PREFIX = "SOUNDTIME-INSTALLER "


def emit(kind, **fields):
    # ASCII framing also works in a remote shell without a UTF-8 locale.
    print(PREFIX + json.dumps({"kind": kind, **fields}), flush=True)


def main():
    request = json.loads(sys.stdin.buffer.readline())
    if platform.system() != "Linux":
        raise ValueError("Choisissez un hôte Linux avec Docker Engine et Compose.")
    action = request["action"]
    settings = request["settings"]
    if action == "check":
        result = engine.preflight(settings)
    elif action in ("install", "lan"):
        operation = engine.install if action == "install" else engine.enable_lan
        result = operation(settings, lambda stage, message: emit("event", stage=stage, message=message))
    else:
        raise ValueError("Action distante inconnue.")
    emit("result", result=result)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, RuntimeError) as exc:
        emit("error", message=str(exc))
    except OSError:
        emit("error", message="Le serveur n’a pas pu accéder au dossier ou à Docker. Vérifiez les droits du compte SSH.")
    except Exception:
        emit("error", message="L’agent distant a rencontré une erreur. Relancez la vérification.")
