"""Build a self-contained Docker Compose project for a remote Linux host."""

import io
import json
import re
import secrets
import shlex
import zipfile
from pathlib import PurePosixPath

if __package__:
    from .guide_strings import SECTIONS
else:
    from guide_strings import SECTIONS


TARGETS = ("nas", "server", "vps", "vm", "cloud")


def _bind(source, target):
    return {"type": "bind", "source": str(source).replace("$", "$$"), "target": target,
            "read_only": True, "bind": {"create_host_path": False}}


def _model(s, project, storage_env, nas_guard):
    path = PurePosixPath(s["directory"])
    web_port = {"target": 80, "published": str(s["port"]), "protocol": "tcp",
                "host_ip": "0.0.0.0" if nas_guard else "${SOUNDTIME_BIND_IP:-127.0.0.1}"}
    nginx_mounts = [_bind(path / "docker/nginx.conf", "/etc/nginx/conf.d/default.conf")]
    if nas_guard:
        nginx_mounts.append(_bind(path / "docker/access", "/etc/nginx/soundtime-bootstrap"))
    media_mounts = ["music_data:/data/music", "p2p_data:/data/p2p-blobs"]
    if s["musicDirectory"]:
        media_mounts.append(_bind(s["musicDirectory"], "/data/music/collection"))
    services = {
        "postgres": {
            "image": "pgvector/pgvector:pg16",
            "environment": {"POSTGRES_USER": "soundtime", "POSTGRES_DB": "soundtime",
                            "POSTGRES_PASSWORD": "${POSTGRES_PASSWORD:?}"},
            "volumes": ["postgres_data:/var/lib/postgresql/data"],
            "healthcheck": {"test": ["CMD", "pg_isready", "-U", "soundtime", "-d", "soundtime"],
                            "interval": "5s", "timeout": "5s", "retries": 12},
        },
        "redis": {
            "image": "redis:7-alpine", "command": ["redis-server", "--maxmemory", "128mb",
                                                    "--maxmemory-policy", "allkeys-lru"],
            "volumes": ["redis_data:/data"],
            "healthcheck": {"test": ["CMD", "redis-cli", "ping"], "interval": "5s",
                            "timeout": "5s", "retries": 12},
        },
        "backend": {
            "image": "ghcr.io/ciccada-corp/soundtime-backend:latest",
            "environment": {
                "DATABASE_URL": "postgres://soundtime:${POSTGRES_PASSWORD:?}@postgres:5432/soundtime",
                "JWT_SECRET": "${JWT_SECRET:?}", "REDIS_URL": "redis://redis:6379",
                "SOUNDTIME_DOMAIN": s["domain"], "SOUNDTIME_SCHEME": "http",
                "AUDIO_STORAGE_PATH": "/data/music", "P2P_ENABLED": str(s["p2p"]).lower(),
                "P2P_BIND_PORT": str(s["peerPort"]), "P2P_BLOBS_DIR": "/data/p2p-blobs",
                "P2P_SECRET_KEY_PATH": "/data/p2p-blobs/secret_key", **storage_env,
            },
            "volumes": media_mounts,
            "depends_on": {name: {"condition": "service_healthy"} for name in ("postgres", "redis")},
        },
        "frontend": {"image": "ghcr.io/ciccada-corp/soundtime-frontend:latest", "depends_on": ["backend"]},
        "nginx": {"image": "nginx:alpine", "volumes": nginx_mounts, "ports": [web_port],
                  "depends_on": ["frontend", "backend"]},
    }
    if s["p2p"]:
        services["backend"]["ports"] = [{"target": s["peerPort"], "published": str(s["peerPort"]), "protocol": "udp"}]
    for service in services.values():
        service["restart"] = "unless-stopped"
    return {"name": project, "services": services,
            "volumes": {name: {} for name in ("postgres_data", "redis_data", "music_data", "p2p_data")}}


def _guard(nginx, password):
    try:
        import bcrypt
    except ModuleNotFoundError as exc:
        if exc.name != "bcrypt":
            raise
        raise RuntimeError("For NAS LAN exports from source, install installer/requirements.txt.") from exc
    cookie = secrets.token_urlsafe(32)
    # Basic auth is needed only until this browser has authenticated once.
    # The cookie leaves Authorization available for SoundTime's Bearer tokens.
    nginx = (f'map $cookie_soundtime_bootstrap $soundtime_realm {{\n    default "SoundTime";\n'
             f'    "{cookie}" off;\n}}\n\n' + nginx)
    marker = "    server_name _;"
    if nginx.count(marker) != 1:
        raise RuntimeError("This Nginx configuration cannot protect the first account during setup.")
    nginx = nginx.replace(marker, marker + "\n    include /etc/nginx/soundtime-bootstrap/*.active;")
    rule = ("auth_basic $soundtime_realm;\nauth_basic_user_file /etc/nginx/soundtime-bootstrap/users;\n"
            f'add_header Set-Cookie "soundtime_bootstrap={cookie}; Path=/; HttpOnly; SameSite=Strict";\n')
    digest = bcrypt.hashpw(password.encode("ascii"), bcrypt.gensalt(rounds=12))
    return nginx, {"docker/access/bootstrap.active": rule.encode(),
                   "docker/access/users": b"soundtime:" + digest + b"\n"}


def _instructions(s, target, password, project):
    def msg(key, **values):
        return SECTIONS[key].format(**values)

    directory = s["directory"]
    port = s["port"]
    sections = [msg("title_" + target), "", msg(target), msg("requirements"), "",
                msg("transfer"), msg("extract"), directory, msg("credentials"), "", msg("start")]
    if target == "nas":
        sections.extend([msg("synology"), msg("qnap", directory=directory), ""])
    sections.extend([msg("terminal"), "  cd " + shlex.quote(directory),
                     "  docker compose config --quiet", "  docker compose pull",
                     "  docker compose up -d", "  docker compose ps", "", msg("portainer", project=project),
                     "", msg("account"), msg("cli_setup")])
    if password:
        sections.extend([msg("nas_guard", port=port), "  " + msg("login"),
                         "  " + msg("password", password=password), msg("private"), msg("unlock")])
    else:
        sections.extend([msg("tunnel"),
                         f"  ssh -N -L 127.0.0.1:{port}:127.0.0.1:{port} USER@HOST",
                         msg("setup_url", port=port), msg("private")])
        if s["access"] == "lan":
            sections.append(msg("lan", port=port))
    if s["storage"]["kind"] == "s3":
        music = msg("s3", bucket=s["storage"]["bucket"])
    elif s["musicDirectory"]:
        music = msg("collection", folder=s["musicDirectory"])
    else:
        music = msg("upload")
    sections.extend(["", msg("music"), music, "", msg("keep"), msg("backup"), msg("logs"), ""])
    return "\n".join(sections)


def archive(s, target, root, storage_env, secret_variables, *, language="en"):
    if not isinstance(target, str) or target not in TARGETS:
        raise ValueError("Choose NAS, dedicated server, VPS, virtual machine or cloud as the destination.")
    project = "soundtime-" + secrets.token_hex(8)
    guard = target == "nas" and s["access"] == "lan"
    values = {"COMPOSE_PROJECT_NAME": project, "SOUNDTIME_BIND_IP": "127.0.0.1",
              "POSTGRES_PASSWORD": secrets.token_hex(32), "JWT_SECRET": secrets.token_hex(32)}
    env = "".join(f"{key}={value}\n" for key, value in values.items()) + secret_variables
    if s["storage"]["kind"] == "s3":
        values.update(S3_ACCESS_KEY=s["storage"]["accessKey"], S3_SECRET_KEY=s["storage"]["secretKey"])
    model = _model(s, project, storage_env, guard)
    # Reuse the deployment defaults shipped in the same installer. Keep the
    # port and enable switch chosen during setup authoritative for this export.
    base_compose = (root / "docker-compose.yml").read_text(encoding="utf-8")
    backend_env = model["services"]["backend"]["environment"]
    for name, default in re.findall(r"^      (P2P_[A-Z0-9_]+): (\$\{\1:-[^}]*\})$", base_compose, re.M):
        backend_env.setdefault(name, default)
    files = {".env": env.encode(), "compose.yaml": (json.dumps(model, indent=2, ensure_ascii=False) + "\n").encode()}
    if target == "nas":
        if __package__:
            from . import qnap
        else:
            import qnap
        files.update(qnap.variant(s, model, values))
    nginx = (root / "docker/nginx.conf").read_text(encoding="utf-8")
    password = secrets.token_urlsafe(28) if guard else None
    if password:
        nginx, auth_files = _guard(nginx, password)
        files.update(auth_files)
    files["docker/nginx.conf"] = nginx.encode()
    files["README.txt"] = _instructions(s, target, password, project).encode()
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as packed:
        for name, data in files.items():
            info = zipfile.ZipInfo(name)
            info.create_system = 3
            # The nginx worker reads the password digest with its non-root UID.
            # .env, the cleartext guide and service environment files stay private.
            mode = 0o644 if name == "docker/access/users" else 0o600
            info.external_attr = (0o100000 | mode) << 16
            packed.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED)
    return f"soundtime-{target}.zip", buffer.getvalue()
