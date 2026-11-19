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
    from . import diagnostics, engine, i18n, onboarding, remote
    from .cli_strings import ROWS
else:
    import diagnostics
    import engine
    import i18n
    import onboarding
    import remote
    from cli_strings import ROWS


SYSTEMS = {"Darwin": "macos", "Windows": "windows", "Linux": "linux"}
DESTINATIONS = ("macos", "windows", "nas", "linux")
EXPORT_TARGETS = ("nas", "server", "vps", "vm", "cloud")
TARGETS = (*DESTINATIONS, "local", "auto", "server", "vps", "vm", "cloud")


class UsageError(Exception):
    pass


class Parser(argparse.ArgumentParser):
    def __init__(self, *args, tr, **kwargs):
        super().__init__(*args, add_help=False, allow_abbrev=False, **kwargs)
        self.tr = tr
        self._optionals.title = tr("options")
        self.add_argument("-h", "--help", action="help", help=tr("help"))

    def format_usage(self):
        return super().format_usage().replace("usage:", self.tr("usage") + ":", 1)

    def format_help(self):
        return super().format_help().replace("usage:", self.tr("usage") + ":", 1)

    def error(self, message):
        # argparse normally repeats unknown arguments verbatim, including a
        # credential mistakenly pasted where an option name was expected.
        raise UsageError(self.tr("invalid_args"))


def _parser(tr):
    parser = Parser(prog="soundtime", tr=tr, description=tr("title"), epilog=tr("secrets"))

    def shared(p):
        p.add_argument("--lang", choices=tuple(i18n.LANGUAGES), default=argparse.SUPPRESS, help=tr("lang"))
        p.add_argument("--json", action="store_true", default=argparse.SUPPRESS, help=tr("json"))

    shared(parser)
    subs = parser.add_subparsers(dest="command", title=tr("commands"))
    for command in ("wizard", "check", "install", "setup", "enable-lan", "export"):
        p = subs.add_parser(command, tr=tr, help=tr(command), description=tr(command), epilog=tr("secrets"))
        shared(p)
        if command == "wizard":
            continue
        choices = EXPORT_TARGETS if command == "export" else TARGETS
        p.add_argument("--target", choices=choices, required=command == "export", help=tr("target"))
        if command != "export":
            for key in ("ssh-host", "ssh-user"):
                p.add_argument("--" + key, metavar=key.upper().replace("-", "_"), help=tr(key))
            p.add_argument("--ssh-port", type=int, metavar="PORT", default=22, help=tr("ssh-port"))
        p.add_argument("--port", type=int, metavar="PORT", help=tr("port"))
        p.add_argument("--directory", metavar="PATH", help=tr("directory"))
        peer = p.add_mutually_exclusive_group()
        peer.add_argument("--p2p", dest="p2p", action="store_true", default=None, help=tr("p2p"))
        peer.add_argument("--no-p2p", dest="p2p", action="store_false", default=None, help=tr("no_p2p"))
        if command == "setup":
            for key in ("admin-user", "admin-email", "instance-name"):
                p.add_argument("--" + key, default="SoundTime" if key == "instance-name" else None,
                               metavar=key.upper().replace("-", "_"), help=tr(key))
            p.add_argument("--public", action="store_true", help=tr("public"))
            p.add_argument("--open-registrations", action="store_true", help=tr("registrations"))
            continue
        p.add_argument("--music-directory", metavar="PATH", help=tr("music-directory"))
        p.add_argument("--domain", metavar="HOST", help=tr("domain"))
        p.add_argument("--access", choices=("local", "lan"), default="lan" if command == "enable-lan" else None, help=tr("access"))
        p.add_argument("--peer-port", type=int, metavar="PORT", help=tr("peer-port"))
        p.add_argument("--storage", choices=("local", "s3"), help=tr("storage"))
        for key in ("s3-endpoint", "s3-bucket", "s3-region", "s3-prefix"):
            p.add_argument("--" + key, metavar="VALUE", help=tr(key))
        if command == "export":
            p.add_argument("--output", metavar="FILE", help=tr("output"))
    return parser


def _ask(prompt, default=""):
    value = input(f"{prompt} [{default}] " if default else f"{prompt} ").strip()
    return value or default


def _accent(value):
    color = (sys.stdout.isatty() and "NO_COLOR" not in os.environ and os.environ.get("TERM") != "dumb"
             and (os.name != "nt" or os.environ.get("WT_SESSION") or os.environ.get("TERM")))
    return f"\033[1;36m{value}\033[0m" if color else value


def _choice(tr, key, items, default=0):
    print("\n" + _accent(tr(key)))
    for i, item in enumerate(items, 1):
        print(f"  {i}. {tr(item)}")
    while True:
        answer = _ask(tr("choose", default=default + 1), str(default + 1))
        if answer.isascii() and answer.isdigit() and len(answer) < 4 and 1 <= int(answer) <= len(items):
            return items[int(answer) - 1]
        print(tr("bad_choice"), file=sys.stderr)


def _port(tr, key, default):
    while True:
        value = _ask(tr(key), str(default))
        if value.isascii() and value.isdigit() and len(value) <= 5 and 1 <= int(value) <= 65535:
            return int(value)
        print(tr("number"), file=sys.stderr)


def _secret(tr, variable, key, interactive):
    value = os.environ.get(variable)
    if value:
        return value
    if interactive and sys.stdin.isatty():
        try:
            # A redirected terminal cannot silently fall back to echoing input.
            with warnings.catch_warnings():
                warnings.simplefilter("error", getpass.GetPassWarning)
                return getpass.getpass(tr(key) + " ")
        except getpass.GetPassWarning:
            pass
    raise ValueError(tr("missing_secret", variable=variable))


def _ssh(args, tr):
    host, user, port = args.ssh_host, args.ssh_user, args.ssh_port
    system = SYSTEMS.get(platform.system())
    if args.target == "nas" and not host and not user:
        raise UsageError(tr("nas_export"))
    if args.target == "local" and (host or user or port != 22):
        raise UsageError(tr("local_ssh"))
    if host or user or port != 22 or args.target not in ("local", system):
        if not host or not user:
            raise UsageError(tr("require_ssh"))
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


def _settings(args, ssh, tr, interactive):
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
            raise UsageError(tr("invalid_args"))
        plan["storage"] = {"kind": "local"}
    elif args.storage == "s3" or storage_options:
        saved = plan["storage"]
        public = {"region": "us-east-1", "bucket": "", "prefix": "", "endpoint": ""}
        public.update({k: saved[k] for k in public if k in saved})
        public.update(storage_options)
        supplied_keys = os.environ.get("SOUNDTIME_S3_ACCESS_KEY") or os.environ.get("SOUNDTIME_S3_SECRET_KEY")
        if not (saved.get("reuseCredentials") and not supplied_keys and all(public[k] == saved[k] for k in public)):
            plan["storage"] = {"kind": "s3", **public,
                               "accessKey": _secret(tr, "SOUNDTIME_S3_ACCESS_KEY", "accessKey", interactive),
                               "secretKey": _secret(tr, "SOUNDTIME_S3_SECRET_KEY", "secretKey", interactive)}
    return plan


def _save_archive(settings, target, output, lang, tr):
    filename, data = engine.export_project(settings, target, language=lang)
    path = Path(output or filename).expanduser().absolute()
    try:
        # O_EXCL also protects dangling symlinks and concurrent exports. The
        # archive contains passwords and is private from the first write.
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError:
        raise ValueError(tr("exists", path=path)) from None
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
    except BaseException:
        path.unlink(missing_ok=True)
        raise
    return {"ok": True, "archive": str(path), "target": target}


def _checks(settings, ssh, lang):
    result = remote.preflight(ssh, settings) if ssh else engine.preflight(settings)
    return {**result, "checks": [{**check, "label": diagnostics.localize(lang, check["label"]),
                                  "detail": diagnostics.localize(lang, check["detail"])} for check in result["checks"]]}


def _profile(args, tr, interactive):
    user = args.admin_user
    email = args.admin_email
    if interactive:
        user = user or _ask(tr("admin-user"), "admin")
        email = email or _ask(tr("admin-email"))
    profile = {"username": user, "email": email, "instanceName": args.instance_name, "p2p": args.p2p,
               "private": not args.public, "openRegistrations": args.open_registrations}
    if not user or not email:
        raise UsageError(tr("profile"))
    profile["password"] = _secret(tr, "SOUNDTIME_ADMIN_PASSWORD", "password", interactive)
    profile["bootstrapPassword"] = os.environ.get("SOUNDTIME_BOOTSTRAP_PASSWORD", "")
    return profile


def _setup(args, ssh, client, lang, tr, interactive):
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
        raise UsageError(tr("number"))
    if ssh:
        port = client.open_tunnel(ssh, port)
    return {"ok": True, **onboarding.configure(port, lambda: _profile(args, tr, interactive), lang,
                                               bootstrap_password=os.environ.get("SOUNDTIME_BOOTSTRAP_PASSWORD"))}


def _deploy(settings, ssh, client, action, lang):
    def report(stage, message):
        print(diagnostics.localize(lang, message), file=sys.stderr, flush=True)
    if ssh:
        result = client.run(ssh, action, settings, report)
    else:
        operation = engine.enable_lan if action == "lan" else engine.install
        result = operation(settings, report)
    result = {"ok": True, **result}
    if result.get("tunnelError"):
        result["tunnelError"] = diagnostics.localize(lang, result["tunnelError"])
    return result


def _closed_result(result):
    # The handle is scoped to this command. Only advertise a forwarded URL
    # alongside the exact command that will reopen the tunnel on the same port.
    if result.get("ssh"):
        result = {**result, "url": "", "tunnelOpen": False}
    return result


def _show(result, tr):
    if "checks" in result:
        for c in result["checks"]:
            print(f"{'✓' if c['ok'] else '×'} {c['label']} — {c['detail']}")
        if any(c["id"] == "docker" and not c["ok"] for c in result["checks"]):
            print(tr("docker_help") + "  https://docs.docker.com/get-started/get-docker/")
    elif "archive" in result:
        print(tr("new_zip", path=result["archive"]))
    elif "setupComplete" in result:
        print(tr("done_setup"))
    else:
        print(tr("success"))
        if result.get("setupTunnel"):
            if result.get("tunnelError"):
                print(result["tunnelError"])
            print(tr("reconnect") + "\n  " + result["setupTunnel"])
        url = result.get("lanUrl") or result.get("url") or result.get("setupManualUrl")
        if url:
            print(tr("url") + "  " + url)
            if url.endswith("/setup"):
                print(tr("need_setup"))
        if result.get("lanPending"):
            print(tr("pending"))


def _remote_destination(lang, tr):
    while True:
        credentials = {"host": _ask(tr("ssh-host")), "user": _ask(tr("ssh-user")),
                       "port": _port(tr, "ssh-port", 22)}
        try:
            conn = remote.validate_destination("auto", credentials)
            try:
                detected = remote.detect(conn)
            except RuntimeError as exc:
                if _choice(tr, "first_login", ("no", "yes")) != "yes":
                    raise exc
                if not remote.prepare_login(conn):
                    raise RuntimeError(tr("ssh_failed"))
                detected = remote.detect(conn)
            system = detected["os"]
            print(tr("detected", system=tr(system), arch=detected["arch"]))
            vendor = detected.get("nas")
            if vendor in ("synology", "qnap"):
                print("NAS  " + tr(vendor))
                if _choice(tr, "method", ("archive", "ssh")) == "archive":
                    return "nas", None, vendor
            return system, remote.validate_destination(system, credentials), None
        except (RuntimeError, ValueError) as exc:
            print(diagnostics.localize(lang, str(exc)), file=sys.stderr)
            if _choice(tr, "retry", ("yes", "no")) == "no":
                return None


def _destination(lang, tr):
    if _choice(tr, "where", ("local", "another")) == "another":
        if _choice(tr, "method", ("ssh", "nas_archive")) == "nas_archive":
            return "nas", None, _choice(tr, "vendor", ("synology", "qnap", "other_nas"))
        return _remote_destination(lang, tr)
    name = platform.system()
    system = SYSTEMS.get(name, "local")
    print(tr("detected", system=tr(system) if system != "local" else name, arch=platform.machine()))
    try:
        print(tr("space", gib=shutil.disk_usage(Path.home()).free // 1024**3))
    except OSError:
        pass
    return system, None, None


def _custom_settings(initial, tr, recommend):
    directory = _ask(tr("directory"), initial["directory"])
    settings = (recommend(directory) if directory != initial["directory"] and recommend else initial.copy())
    settings["directory"] = directory
    settings["port"] = _port(tr, "port", settings["port"])
    settings["domain"] = _ask(tr("domain"), settings["domain"])
    access = _choice(tr, "access", ("loopback", "lan"), int(settings["access"] == "lan"))
    settings["access"] = "lan" if access == "lan" else "local"
    settings["p2p"] = _choice(tr, "p2p", ("no", "yes"), int(settings["p2p"])) == "yes"
    if settings["p2p"]:
        settings["peerPort"] = _port(tr, "peer-port", settings["peerPort"])
    storage = settings["storage"]
    if _choice(tr, "storage", ("disk", "s3"), int(storage["kind"] == "s3")) == "s3":
        settings["musicDirectory"] = ""
        if not storage.get("reuseCredentials") or _choice(tr, "keep_s3", ("yes", "no")) == "no":
            settings["storage"] = {
                "kind": "s3", "bucket": _ask(tr("s3-bucket"), storage.get("bucket", "")),
                "endpoint": _ask(tr("s3-endpoint"), storage.get("endpoint", "")),
                "region": _ask(tr("s3-region"), storage.get("region", "us-east-1")),
                "prefix": _ask(tr("s3-prefix"), storage.get("prefix", "")),
                "accessKey": _secret(tr, "SOUNDTIME_S3_ACCESS_KEY", "accessKey", True),
                "secretKey": _secret(tr, "SOUNDTIME_S3_SECRET_KEY", "secretKey", True),
            }
    else:
        settings["storage"] = {"kind": "local"}
        settings["musicDirectory"] = _ask(tr("music-directory"), settings["musicDirectory"])
    return settings


def _review(settings, target, ssh, tr):
    print("\n" + _accent(tr("review")))
    storage = settings["storage"]
    rows = [("target", tr(target) + (f" · {ssh['user']}@{ssh['host']}" if ssh else "")),
            ("directory", settings["directory"]), ("domain", settings["domain"]), ("port", settings["port"]),
            ("access", tr("nas_guard" if target == "nas" and settings["access"] == "lan"
                          else "lan" if settings["access"] == "lan" else "loopback")),
            ("storage", tr("disk") if storage["kind"] == "local" else f"S3 · {storage['bucket']} · {storage['region']}"),
            ("p2p", f"UDP {settings['peerPort']}" if settings["p2p"] else tr("no"))]
    if settings["musicDirectory"]:
        rows.append(("music-directory", settings["musicDirectory"]))
    for key, value in rows:
        print(f"  {tr(key)}  {value}")


def _wizard_setup(settings, ssh, client, result, lang, tr):
    if not result.get("url", "").endswith("/setup") or _choice(tr, "configure", ("yes", "no")) == "no":
        return result
    while True:
        try:
            args = argparse.Namespace(admin_user=None, admin_email=None, instance_name=_ask(tr("instance-name"), "SoundTime"),
                                      p2p=settings["p2p"], public=False, open_registrations=False)
            profile = _profile(args, tr, True)
            profile["private"] = _choice(tr, "public", ("no", "yes")) != "yes"
            if not profile["private"]:
                profile["openRegistrations"] = _choice(tr, "registrations", ("no", "yes")) == "yes"
            onboarding.configure(client.local_port if ssh else settings["port"], profile, lang)
            break
        except (ValueError, RuntimeError) as exc:
            print(diagnostics.localize(lang, str(exc)), file=sys.stderr)
            if _choice(tr, "retry", ("yes", "no")) != "yes":
                raise
    print(tr("done_setup"))
    result["url"] = result["url"].removesuffix("setup")
    if result.get("setupManualUrl"):
        result["setupManualUrl"] = result["setupManualUrl"].removesuffix("setup")
    if result.get("lanPending") and _choice(tr, "publish", ("yes", "no")) == "yes":
        return _deploy(settings, ssh, client, "lan", lang)
    return result


def _wizard(lang, tr):
    if sys.stdout.isatty():
        print("\n" + _accent("◉  SoundTime"))
    selection = _destination(lang, tr)
    if not selection:
        return 1
    target, ssh, vendor = selection
    recommend = None
    if vendor:
        dirs = {"synology": "/volume1/docker/soundtime", "qnap": "/share/Container/soundtime", "other_nas": "/opt/soundtime"}
        initial = {**engine.defaults(), "directory": dirs[vendor], "access": "lan"}
    else:
        recommend = (lambda directory=None: remote.recommend(ssh, directory=directory)) if ssh else engine.recommend
        print(tr("inspect"))
        try:
            initial = recommend()
        except (ValueError, RuntimeError) as exc:
            print(diagnostics.localize(lang, str(exc)), file=sys.stderr)
            initial = recommend(_ask(tr("directory")))
    with contextlib.closing(remote.RemoteInstaller()) as client:
        while True:
            custom = _choice(tr, "mode", ("quick", "custom")) == "custom"
            try:
                settings = _custom_settings(initial, tr, recommend) if custom else initial.copy()
            except (ValueError, RuntimeError) as exc:
                print(diagnostics.localize(lang, str(exc)), file=sys.stderr)
                if _choice(tr, "retry", ("yes", "no")) == "yes":
                    continue
                return 1
            _review(settings, target, ssh, tr)
            if vendor:
                output = _ask(tr("output"), f"soundtime-{target}.zip")
            else:
                checks = _checks(settings, ssh, lang)
                _show(checks, tr)
                if not checks["ok"]:
                    if _choice(tr, "retry", ("yes", "no")) == "yes":
                        continue
                    return 1
            if _choice(tr, "confirm", ("yes", "no")) != "yes":
                print(tr("cancelled"))
                return 0
            try:
                if vendor:
                    result = _save_archive(settings, target, output, lang, tr)
                    _show(result, tr)
                    print(tr("nas_" + vendor))
                    return 0
                result = _deploy(settings, ssh, client, "install", lang)
            except (OSError, ValueError, RuntimeError) as exc:
                msg = tr("filesystem") if isinstance(exc, OSError) else diagnostics.localize(lang, str(exc))
                print(msg, file=sys.stderr)
                if _choice(tr, "retry", ("yes", "no")) == "yes":
                    continue
                return 1
            result = _wizard_setup(settings, ssh, client, result, lang, tr)
            _show(_closed_result(result), tr)
            return 0


def main(argv=None):
    if sys.platform == "win32":
        for stream in (sys.stdout, sys.stderr):
            if hasattr(stream, "reconfigure") and not stream.isatty():
                stream.reconfigure(encoding="utf-8", errors="replace")
    args_in = list(sys.argv[1:] if argv is None else argv)
    explicit = None
    for i, token in enumerate(args_in):
        if token == "--lang" and i + 1 < len(args_in):
            explicit = args_in[i + 1]
        elif token.startswith("--lang="):
            explicit = token.split("=", 1)[1]
    lang = i18n.language(explicit if explicit in i18n.LANGUAGES else None)
    tr = lambda key, **values: i18n.text(lang, ROWS, key, **values)
    json_mode = "--json" in args_in
    try:
        try:
            args = _parser(tr).parse_args(args_in)
        except SystemExit as exc:
            return exc.code
        command = args.command or "wizard"
        if command == "wizard":
            if json_mode:
                raise UsageError(tr("no_json_wizard"))
            return _wizard(lang, tr)
        if command != "export" and args.target is None:
            args.target = "auto" if args.ssh_host or args.ssh_user else "local"
        ssh = _ssh(args, tr) if command != "export" else None
        interactive = not json_mode and sys.stdin.isatty()
        with contextlib.closing(remote.RemoteInstaller()) as client:
            if command == "setup":
                result = _setup(args, ssh, client, lang, tr, interactive)
            else:
                settings = _settings(args, ssh, tr, interactive)
                if command == "export":
                    result = _save_archive(settings, args.target, args.output, lang, tr)
                elif command == "check":
                    result = _checks(settings, ssh, lang)
                else:
                    result = _closed_result(_deploy(settings, ssh, client, "lan" if command == "enable-lan" else "install", lang))
        if json_mode:
            print(json.dumps(result, ensure_ascii=False))
        else:
            _show(result, tr)
        return 0 if result["ok"] else 1
    except (KeyboardInterrupt, EOFError, OSError, RuntimeError, ValueError, UsageError) as exc:
        if isinstance(exc, KeyboardInterrupt):
            code, message = 130, tr("cancelled")
        elif isinstance(exc, EOFError):
            code, message = 2, tr("no_input")
        elif isinstance(exc, UsageError):
            code, message = 2, str(exc)
        elif isinstance(exc, OSError):
            code, message = 1, tr("unavailable")
        else:
            code, message = 1, diagnostics.localize(lang, str(exc))
        if json_mode:
            print(json.dumps({"ok": False, "error": message}, ensure_ascii=False))
        else:
            print(message, file=sys.stderr)
        return code


if __name__ == "__main__":
    raise SystemExit(main())
