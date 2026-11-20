"""Check only network membership of the four containers in this disposable run."""
import json
import subprocess
import sys


def inspect(args):
    return subprocess.check_output(["docker", *args], text=True).splitlines()


def main(ids):
    if len(ids) != 4 or len(set(ids)) != 4 or not all(ids):
        raise ValueError("Expected four distinct, running federation containers")
    config = inspect(["container", "inspect", "--format", "{{json .NetworkSettings.Networks}}", *ids])
    networks = [set(json.loads(line)) for line in config]
    backends, relay = networks[:3], networks[3]
    if (any(len(n) != 1 for n in backends) or set.union(*backends) != relay or len(relay) != 3):
        raise ValueError("A backend can bypass the private relay or cannot reach it")
    flags = inspect(["network", "inspect", "--format", "{{.Internal}}", *sorted(relay)])
    if flags != ["true"] * 3:
        raise ValueError("Expected three internal Docker networks")
    print(json.dumps(dict(zip(["A", "B", "C", "relay"], [sorted(n) for n in networks])), indent=2))


if __name__ == "__main__":
    main(sys.argv[1:])
