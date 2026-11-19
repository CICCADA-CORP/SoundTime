"""Console entry point for source checkouts and portable executables."""
import sys
from pathlib import Path

if __package__ in ("", None):
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from installer import cli, remote_agent


def main(argv=None):
    args = sys.argv[1:] if argv is None else argv
    if args == ["--stdio-agent"]:
        return remote_agent.main()
    return cli.main(args)


if __name__ == "__main__":
    raise SystemExit(main())
