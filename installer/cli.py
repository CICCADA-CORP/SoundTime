"""SoundTime terminal installer. All credentials use protected input or the environment."""

import argparse
import contextlib
import getpass
import json
import os
import platform
import shutil
import sys
import warnings
from pathlib import Path

if __package__:
    from . import diagnostics, engine, onboarding, remote
    from .cli_strings import text
else:
    import diagnostics
    import engine
    import onboarding
    import remote
    from cli_strings import text


SYSTEMS = {"Darwin": "macos", "Windows": "windows", "Linux": "linux"}
DESTINATIONS = ("macos", "windows", "nas", "linux")
EXPORT_TARGETS = ("nas", "server", "vps", "vm", "cloud")
TARGETS = (*DESTINATIONS, "local", "auto", "server", "vps", "vm", "cloud")


class UsageError(Exception):
    pass


class EnglishHelpFormatter(argparse.HelpFormatter):
    def add_usage(self, usage, actions, groups, prefix=None):
        # argparse otherwise translates the usage heading using system gettext.
        super().add_usage(usage, actions, groups, "Usage: " if prefix is None else prefix)


class Parser(argparse.ArgumentParser):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, add_help=False, allow_abbrev=False,
                         formatter_class=EnglishHelpFormatter, **kwargs)
        self._optionals.title = text("options")
        self.add_argument("-h", "--help", action="help", help=text("help"))

    def error(self, message):
        # argparse normally repeats unknown arguments verbatim, including a
        # credential mistakenly pasted where an option name was expected.
        raise UsageError(text("invalid_args"))


def _legacy_language(value):
    code = value.strip().lower().split(".", 1)[0].split("@", 1)[0].replace("_", "-").split("-", 1)[0]
    if code not in ("en", "fr", "es", "ru", "zh"):
        raise ValueError
    return "en"


def _parser():
    parser = Parser(prog="soundtime", description=text("title"), epilog=text("secrets"))

    def shared(p):
        # Accept flags used by previous releases without presenting a language selector.
        p.add_argument("--lang", type=_legacy_language, default=argparse.SUPPRESS, help=argparse.SUPPRESS)
        p.add_argument("--json", action="store_true", default=argparse.SUPPRESS, help=text("json"))

    shared(parser)
    subs = parser.add_subparsers(dest="command", title=text("commands"))
    for command in ("wizard", "check", "install", "setup", "enable-lan", "export"):
        p = subs.add_parser(command, help=text(command), description=text(command), epilog=text("secrets"))
        shared(p)
        if command == "wizard":
            continue
        choices = EXPORT_TARGETS if command == "export" else TARGETS
        p.add_argument("--target", choices=choices, required=command == "export", help=text("target"))
        if command != "export":
            for key in ("ssh-host", "ssh-user"):
                p.add_argument("--" + key, metavar=key.upper().replace("-", "_"), help=text(key))
            p.add_argument("--ssh-port", type=int, metavar="PORT", default=22, help=text("ssh-port"))
        p.add_argument("--port", type=int, metavar="PORT", help=text("port"))
        p.add_argument("--directory", metavar="PATH", help=text("directory"))
        peer = p.add_mutually_exclusive_group()
        peer.add_argument("--p2p", dest="p2p", action="store_true", default=None, help=text("p2p"))
        peer.add_argument("--no-p2p", dest="p2p", action="store_false", default=None, help=text("no_p2p"))
        if command == "setup":
            for key in ("admin-user", "admin-email", "instance-name"):
                p.add_argument("--" + key, default="SoundTime" if key == "instance-name" else None,
                               metavar=key.upper().replace("-", "_"), help=text(key))
            p.add_argument("--public", action="store_true", help=text("public"))
            p.add_argument("--open-registrations", action="store_true", help=text("registrations"))
            continue
        p.add_argument("--music-directory", metavar="PATH", help=text("music-directory"))
        p.add_argument("--domain", metavar="HOST", help=text("domain"))
        p.add_argument("--access", choices=("local", "lan"), default="lan" if command == "enable-lan" else None, help=text("access"))
        p.add_argument("--peer-port", type=int, metavar="PORT", help=text("peer-port"))
        p.add_argument("--storage", choices=("local", "s3"), help=text("storage"))
        for key in ("s3-endpoint", "s3-bucket", "s3-region", "s3-prefix"):
            p.add_argument("--" + key, metavar="VALUE", help=text(key))
        if command == "export":
            p.add_argument("--output", metavar="FILE", help=text("output"))
    return parser


def _ask(prompt, default=""):
    value = input(f"{prompt} [{default}] " if default else f"{prompt} ").strip()
    return value or default


def _accent(value):
    color = (sys.stdout.isatty() and "NO_COLOR" not in os.environ and os.environ.get("TERM") != "dumb"
             and (os.name != "nt" or os.environ.get("WT_SESSION") or os.environ.get("TERM")))
    return f"\033[1;36m{value}\033[0m" if color else value


def _choice(key, items, default=0):
    print("\n" + _accent(text(key)))
    for i, item in enumerate(items, 1):
        print(f"  {i}. {text(item)}")
    while True:
        answer = _ask(text("choose", default=default + 1), str(default + 1))
        if answer.isascii() and answer.isdigit() and len(answer) < 4 and 1 <= int(answer) <= len(items):
            return items[int(answer) - 1]
        print(text("bad_choice"), file=sys.stderr)


def _port(key, default):
    while True:
        value = _ask(text(key), str(default))
        if value.isascii() and value.isdigit() and len(value) <= 5 and 1 <= int(value) <= 65535:
            return int(value)
        print(text("number"), file=sys.stderr)


def _secret(variable, key, interactive):
    value = os.environ.get(variable)
    if value:
        return value
    if interactive and sys.stdin.isatty():
        try:
            # A redirected terminal cannot silently fall back to echoing input.
            with warnings.catch_warnings():
                warnings.simplefilter("error", getpass.GetPassWarning)
                return getpass.getpass(text(key) + " ")
        except getpass.GetPassWarning:
            pass
    raise ValueError(text("missing_secret", variable=variable))


def _ssh(args):
    host, user, port = args.ssh_host, args.ssh_user, args.ssh_port
    system = SYSTEMS.get(platform.system())
    if args.target == "nas" and not host and not user:
        raise UsageError(text("nas_export"))
    if args.target == "local" and (host or user or port != 22):
        raise UsageError(text("local_ssh"))
    if host or user or port != 22 or args.target not in ("local", system):
        if not host or not user:
            raise UsageError(text("require_ssh"))
        credentials = {"host": host, "user": user, "port": port}
        target = "linux" if args.target == "nas" else args.target
        ssh = remote.validate_destination(target, credentials)
        if target == "auto":
            detected = remote.detect(ssh)
            ssh = remote.validate_destination(detected["os"], credentials)
        return ssh
    return None


def _default_folder(target, export, ssh):
    if export:
        return "/volume1/docker/soundtime" if target == "nas" else "/opt/soundtime"
    if ssh:
        return "~/SoundTime Server"
    return engine.defaults()["directory"]


def _recommended(directory, ssh):
    return remote.recommend(ssh, directory=directory) if ssh else engine.recommend(directory)


def _settings(args, ssh, interactive):
    if args.command == "export":
        plan = {**engine.defaults(), "directory": args.directory or _default_folder(args.target, True, None),
                "access": "lan" if args.target == "nas" else "local"}
    else:
        plan = _recommended(args.directory, ssh)
    for field, override in (("port", args.port), ("peerPort", args.peer_port), ("access", args.access),
                            ("domain", args.domain), ("musicDirectory", args.music_directory), ("p2p", args.p2p)):
        if override is not None:
            plan[field] = override
    storage_options = {key: value for key, value in (("region", args.s3_region), ("bucket", args.s3_bucket),
                       ("prefix", args.s3_prefix), ("endpoint", args.s3_endpoint)) if value is not None}
    if args.storage == "local":
        if storage_options:
            raise UsageError(text("invalid_args"))
        plan["storage"] = {"kind": "local"}
    elif args.storage == "s3" or storage_options:
        saved = plan["storage"]
        public = {"region": "us-east-1", "bucket": "", "prefix": "", "endpoint": ""}
        public.update({k: saved[k] for k in public if k in saved})
        public.update(storage_options)
        supplied_keys = os.environ.get("SOUNDTIME_S3_ACCESS_KEY") or os.environ.get("SOUNDTIME_S3_SECRET_KEY")
        if not (saved.get("reuseCredentials") and not supplied_keys and all(public[k] == saved[k] for k in public)):
            plan["storage"] = {"kind": "s3", **public,
                               "accessKey": _secret("SOUNDTIME_S3_ACCESS_KEY", "accessKey", interactive),
                               "secretKey": _secret("SOUNDTIME_S3_SECRET_KEY", "secretKey", interactive)}
    return plan


def _save_archive(settings, target, output):
    filename, data = engine.export_project(settings, target)
    path = Path(output or filename).expanduser().absolute()
    try:
        # O_EXCL also protects dangling symlinks and concurrent exports. The
        # archive contains passwords and is private from the first write.
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError:
        raise ValueError(text("exists", path=path)) from None
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
    except BaseException:
        path.unlink(missing_ok=True)
        raise
    return {"ok": True, "archive": str(path), "target": target}


def _checks(settings, ssh):
    result = remote.preflight(ssh, settings) if ssh else engine.preflight(settings)
    return {**result, "checks": [{**check, "label": diagnostics.english(check["label"]),
                                  "detail": diagnostics.english(check["detail"])} for check in result["checks"]]}


def _profile(args, interactive):
    user = args.admin_user
    email = args.admin_email
    if interactive:
        user = user or _ask(text("admin-user"), "admin")
        email = email or _ask(text("admin-email"))
    profile = {"username": user, "email": email, "instanceName": args.instance_name, "p2p": args.p2p,
               "private": not args.public, "openRegistrations": args.open_registrations}
    if not user or not email:
        raise UsageError(text("profile"))
    profile["password"] = _secret("SOUNDTIME_ADMIN_PASSWORD", "password", interactive)
    profile["bootstrapPassword"] = os.environ.get("SOUNDTIME_BOOTSTRAP_PASSWORD", "")
    return profile


def _setup(args, ssh, client, interactive):
    if args.port is None:
        saved = _recommended(args.directory, ssh)
        port = saved["port"]
        if args.p2p is None:
            args.p2p = saved["p2p"]
    else:
        port = args.port
    if args.p2p is None:
        args.p2p = False
    if type(port) is not int or not 1 <= port <= 65535:
        raise UsageError(text("number"))
    if ssh:
        port = client.open_tunnel(ssh, port)
    return {"ok": True, **onboarding.configure(port, lambda: _profile(args, interactive),
                                               bootstrap_password=os.environ.get("SOUNDTIME_BOOTSTRAP_PASSWORD"))}


def _deploy(settings, ssh, client, action):
    def report(stage, message):
        print(diagnostics.english(message), file=sys.stderr, flush=True)
    if ssh:
        result = client.run(ssh, action, settings, report)
    else:
        operation = engine.enable_lan if action == "lan" else engine.install
        result = operation(settings, report)
    result = {"ok": True, **result}
    if result.get("tunnelError"):
        result["tunnelError"] = diagnostics.english(result["tunnelError"])
    return result


def _closed_result(result):
    # The handle is scoped to this command. Only advertise a forwarded URL
    # alongside the exact command that will reopen the tunnel on the same port.
    if result.get("ssh"):
        result = {**result, "url": "", "tunnelOpen": False}
    return result


def _show(result):
    if "checks" in result:
        for c in result["checks"]:
            print(f"{'✓' if c['ok'] else '×'} {c['label']} — {c['detail']}")
        if any(c["id"] == "docker" and not c["ok"] for c in result["checks"]):
            print(text("docker_help") + "  https://docs.docker.com/get-started/get-docker/")
    elif "archive" in result:
        print(text("new_zip", path=result["archive"]))
    elif "setupComplete" in result:
        print(text("done_setup"))
    else:
        print(text("success"))
        if result.get("setupTunnel"):
            if result.get("tunnelError"):
                print(result["tunnelError"])
            print(text("reconnect") + "\n  " + result["setupTunnel"])
        url = result.get("lanUrl") or result.get("url") or result.get("setupManualUrl")
        if url:
            print(text("url") + "  " + url)
            if url.endswith("/setup"):
                print(text("need_setup"))
        if result.get("lanPending"):
            print(text("pending"))


def _remote_destination():
    while True:
        credentials = {"host": _ask(text("ssh-host")), "user": _ask(text("ssh-user")),
                       "port": _port("ssh-port", 22)}
        try:
            conn = remote.validate_destination("auto", credentials)
            try:
                detected = remote.detect(conn)
            except RuntimeError as exc:
                if _choice("first_login", ("no", "yes")) != "yes":
                    raise exc
                if not remote.prepare_login(conn):
                    raise RuntimeError(text("ssh_failed"))
                detected = remote.detect(conn)
            system = detected["os"]
            print(text("detected", system=text(system), arch=detected["arch"]))
            vendor = detected.get("nas")
            if vendor in ("synology", "qnap"):
                print("NAS  " + text(vendor))
                if _choice("method", ("archive", "ssh")) == "archive":
                    return "nas", None, vendor
            return system, remote.validate_destination(system, credentials), None
        except (RuntimeError, ValueError) as exc:
            print(diagnostics.english(str(exc)), file=sys.stderr)
            if _choice("retry", ("yes", "no")) == "no":
                return None


def _destination():
    if _choice("where", ("local", "another")) == "another":
        if _choice("method", ("ssh", "nas_archive")) == "nas_archive":
            return "nas", None, _choice("vendor", ("synology", "qnap", "other_nas"))
        return _remote_destination()
    name = platform.system()
    system = SYSTEMS.get(name, "local")
    print(text("detected", system=text(system) if system != "local" else name, arch=platform.machine()))
    try:
        print(text("space", gib=shutil.disk_usage(Path.home()).free // 1024**3))
    except OSError:
        pass
    return system, None, None


def _custom_settings(initial, recommend):
    directory = _ask(text("directory"), initial["directory"])
    settings = (recommend(directory) if directory != initial["directory"] and recommend else initial.copy())
    settings["directory"] = directory
    settings["port"] = _port("port", settings["port"])
    settings["domain"] = _ask(text("domain"), settings["domain"])
    access = _choice("access", ("loopback", "lan"), int(settings["access"] == "lan"))
    settings["access"] = "lan" if access == "lan" else "local"
    settings["p2p"] = _choice("p2p", ("no", "yes"), int(settings["p2p"])) == "yes"
    if settings["p2p"]:
        settings["peerPort"] = _port("peer-port", settings["peerPort"])
    storage = settings["storage"]
    if _choice("storage", ("disk", "s3"), int(storage["kind"] == "s3")) == "s3":
        settings["musicDirectory"] = ""
        if not storage.get("reuseCredentials") or _choice("keep_s3", ("yes", "no")) == "no":
            settings["storage"] = {
                "kind": "s3", "bucket": _ask(text("s3-bucket"), storage.get("bucket", "")),
                "endpoint": _ask(text("s3-endpoint"), storage.get("endpoint", "")),
                "region": _ask(text("s3-region"), storage.get("region", "us-east-1")),
                "prefix": _ask(text("s3-prefix"), storage.get("prefix", "")),
                "accessKey": _secret("SOUNDTIME_S3_ACCESS_KEY", "accessKey", True),
                "secretKey": _secret("SOUNDTIME_S3_SECRET_KEY", "secretKey", True),
            }
    else:
        settings["storage"] = {"kind": "local"}
        settings["musicDirectory"] = _ask(text("music-directory"), settings["musicDirectory"])
    return settings


def _review(settings, target, ssh):
    print("\n" + _accent(text("review")))
    storage = settings["storage"]
    rows = [("target", text(target) + (f" · {ssh['user']}@{ssh['host']}" if ssh else "")),
            ("directory", settings["directory"]), ("domain", settings["domain"]), ("port", settings["port"]),
            ("access", text("nas_guard" if target == "nas" and settings["access"] == "lan"
                          else "lan" if settings["access"] == "lan" else "loopback")),
            ("storage", text("disk") if storage["kind"] == "local" else f"S3 · {storage['bucket']} · {storage['region']}"),
            ("p2p", f"UDP {settings['peerPort']}" if settings["p2p"] else text("no"))]
    if settings["musicDirectory"]:
        rows.append(("music-directory", settings["musicDirectory"]))
    for key, value in rows:
        print(f"  {text(key)}  {value}")


def _wizard_setup(settings, ssh, client, result):
    if not result.get("url", "").endswith("/setup") or _choice("configure", ("yes", "no")) == "no":
        return result
    while True:
        try:
            args = argparse.Namespace(admin_user=None, admin_email=None, instance_name=_ask(text("instance-name"), "SoundTime"),
                                      p2p=settings["p2p"], public=False, open_registrations=False)
            profile = _profile(args, True)
            profile["private"] = _choice("public", ("no", "yes")) != "yes"
            if not profile["private"]:
                profile["openRegistrations"] = _choice("registrations", ("no", "yes")) == "yes"
            onboarding.configure(client.local_port if ssh else settings["port"], profile)
            break
        except (ValueError, RuntimeError) as exc:
            print(diagnostics.english(str(exc)), file=sys.stderr)
            if _choice("retry", ("yes", "no")) != "yes":
                raise
    print(text("done_setup"))
    result["url"] = result["url"].removesuffix("setup")
    if result.get("setupManualUrl"):
        result["setupManualUrl"] = result["setupManualUrl"].removesuffix("setup")
    if result.get("lanPending") and _choice("publish", ("yes", "no")) == "yes":
        return _deploy(settings, ssh, client, "lan")
    return result


def _wizard():
    if sys.stdout.isatty():
        print("\n" + _accent("◉  SoundTime"))
    selection = _destination()
    if not selection:
        return 1
    target, ssh, vendor = selection
    recommend = None
    if vendor:
        dirs = {"synology": "/volume1/docker/soundtime", "qnap": "/share/Container/soundtime", "other_nas": "/opt/soundtime"}
        initial = {**engine.defaults(), "directory": dirs[vendor], "access": "lan"}
    else:
        recommend = (lambda directory=None: remote.recommend(ssh, directory=directory)) if ssh else engine.recommend
        print(text("inspect"))
        try:
            initial = recommend()
        except (ValueError, RuntimeError) as exc:
            print(diagnostics.english(str(exc)), file=sys.stderr)
            initial = recommend(_ask(text("directory")))
    with contextlib.closing(remote.RemoteInstaller()) as client:
        while True:
            custom = _choice("mode", ("quick", "custom")) == "custom"
            try:
                settings = _custom_settings(initial, recommend) if custom else initial.copy()
            except (ValueError, RuntimeError) as exc:
                print(diagnostics.english(str(exc)), file=sys.stderr)
                if _choice("retry", ("yes", "no")) == "yes":
                    continue
                return 1
            _review(settings, target, ssh)
            if vendor:
                output = _ask(text("output"), f"soundtime-{target}.zip")
            else:
                checks = _checks(settings, ssh)
                _show(checks)
                if not checks["ok"]:
                    if _choice("retry", ("yes", "no")) == "yes":
                        continue
                    return 1
            if _choice("confirm", ("yes", "no")) != "yes":
                print(text("cancelled"))
                return 0
            try:
                if vendor:
                    result = _save_archive(settings, target, output)
                    _show(result)
                    print(text("nas_" + vendor))
                    return 0
                result = _deploy(settings, ssh, client, "install")
            except (OSError, ValueError, RuntimeError) as exc:
                msg = text("filesystem") if isinstance(exc, OSError) else diagnostics.english(str(exc))
                print(msg, file=sys.stderr)
                if _choice("retry", ("yes", "no")) == "yes":
                    continue
                return 1
            result = _wizard_setup(settings, ssh, client, result)
            _show(_closed_result(result))
            return 0


def main(argv=None):
    if sys.platform == "win32":
        for stream in (sys.stdout, sys.stderr):
            if hasattr(stream, "reconfigure") and not stream.isatty():
                stream.reconfigure(encoding="utf-8", errors="replace")
    args_in = list(sys.argv[1:] if argv is None else argv)
    json_mode = "--json" in args_in
    try:
        try:
            args = _parser().parse_args(args_in)
        except SystemExit as exc:
            return exc.code
        command = args.command or "wizard"
        if command == "wizard":
            if json_mode:
                raise UsageError(text("no_json_wizard"))
            return _wizard()
        if command != "export" and args.target is None:
            args.target = "auto" if args.ssh_host or args.ssh_user else "local"
        ssh = _ssh(args) if command != "export" else None
        interactive = not json_mode and sys.stdin.isatty()
        with contextlib.closing(remote.RemoteInstaller()) as client:
            if command == "setup":
                result = _setup(args, ssh, client, interactive)
            else:
                settings = _settings(args, ssh, interactive)
                if command == "export":
                    result = _save_archive(settings, args.target, args.output)
                elif command == "check":
                    result = _checks(settings, ssh)
                else:
                    result = _closed_result(_deploy(settings, ssh, client, "lan" if command == "enable-lan" else "install"))
        if json_mode:
            print(json.dumps(result, ensure_ascii=False))
        else:
            _show(result)
        return 0 if result["ok"] else 1
    except (KeyboardInterrupt, EOFError, OSError, RuntimeError, ValueError, UsageError) as exc:
        if isinstance(exc, KeyboardInterrupt):
            code, message = 130, text("cancelled")
        elif isinstance(exc, EOFError):
            code, message = 2, text("no_input")
        elif isinstance(exc, UsageError):
            code, message = 2, str(exc)
        elif isinstance(exc, OSError):
            code, message = 1, text("unavailable")
        else:
            code, message = 1, diagnostics.english(str(exc))
        if json_mode:
            print(json.dumps({"ok": False, "error": message}, ensure_ascii=False))
        else:
            print(message, file=sys.stderr)
        return code


if __name__ == "__main__":
    raise SystemExit(main())
