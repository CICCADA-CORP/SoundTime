"""Build a self-contained Docker Compose project for a remote Linux host."""

import base64
import hashlib
import io
import json
import secrets
import shlex
import zipfile
from pathlib import PurePosixPath


TARGETS = {"nas": "NAS", "server": "serveur dédié", "vps": "VPS", "vm": "machine virtuelle", "cloud": "serveur cloud"}


def _bind(source, target):
    return {"type": "bind", "source": str(source).replace("$", "$$"), "target": target,
            "read_only": True, "bind": {"create_host_path": False}}


def _model(s, project, storage_env, nas_guard):
    path = PurePosixPath(s["directory"])
    web_port = {"target": 80, "published": str(s["port"]), "protocol": "tcp",
                "host_ip": "0.0.0.0" if nas_guard else "${SOUNDTIME_BIND_IP:-127.0.0.1}"}
    nginx_mounts = [_bind(path / "docker/nginx.conf", "/etc/nginx/conf.d/default.conf")]
    if nas_guard:
        nginx_mounts.append(_bind(path / "docker/acces", "/etc/nginx/soundtime-bootstrap"))
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
    cookie = secrets.token_urlsafe(32)
    # Basic auth is needed only until this browser has authenticated once.
    # The cookie leaves Authorization available for SoundTime's Bearer tokens.
    nginx = (f'map $cookie_soundtime_bootstrap $soundtime_realm {{\n    default "SoundTime";\n'
             f'    "{cookie}" off;\n}}\n\n' + nginx)
    marker = "    server_name _;"
    if nginx.count(marker) != 1:
        raise RuntimeError("Impossible de préparer la protection du premier compte avec cette configuration Nginx.")
    nginx = nginx.replace(marker, marker + "\n    include /etc/nginx/soundtime-bootstrap/*.active;")
    rule = ("auth_basic $soundtime_realm;\nauth_basic_user_file /etc/nginx/soundtime-bootstrap/comptes;\n"
            f'add_header Set-Cookie "soundtime_bootstrap={cookie}; Path=/; HttpOnly; SameSite=Strict";\n')
    digest = base64.b64encode(hashlib.sha1(password.encode("ascii")).digest()).decode("ascii")
    return nginx, {"docker/acces/bootstrap.active": rule.encode(),
                   "docker/acces/comptes": f"soundtime:{{SHA}}{digest}\n".encode()}


def _instructions(s, target, password, project):
    dest = s["directory"]
    port = s["port"]
    intro = {
        "nas": "Dans le gestionnaire de fichiers de votre NAS, créez le dossier du projet. Activez son service de conteneurs Linux.",
        "server": "Connectez-vous au serveur dédié avec le compte qui peut gérer Docker.",
        "vm": "Dans la VM, démarrez Docker. Pour vous y connecter depuis votre ordinateur, configurez une adresse joignable ou une redirection SSH dans l’hyperviseur.",
        "vps": "Connectez-vous en SSH à votre VPS avec votre compte habituel. Autorisez votre accès SSH dans le pare-feu de l’hébergeur.",
        "cloud": "Utilisez une instance Linux avec disque persistant. Son groupe de sécurité doit autoriser SSH depuis votre adresse. Les volumes Docker résident sur le disque de cette instance.",
    }[target]
    guide = f"""Installer SoundTime sur votre {TARGETS[target]}

{intro}
Prévoyez un moteur Docker Linux x86-64 ou ARM64 avec Compose v2, de la place pour les images et la bibliothèque, et un accès à Docker Hub et ghcr.io.

1. Transférer le projet
Décompressez cette archive privée dans le dossier choisi sur la machine cible.
{dest}
Dans ce dossier doivent se trouver compose.yaml, le fichier caché .env et le sous-dossier docker. Limitez l’accès au projet à votre compte, notamment à .env, aux fichiers de clés et à ce guide. Cette archive initialise une nouvelle instance. Pour une reprise ou une mise à jour, gardez le même projet et ses identifiants.

2. Démarrer SoundTime
"""
    if target == "nas":
        guide += f"""Sur un NAS Synology avec Container Manager, ouvrez Projet → Créer. Choisissez le dossier du projet et son fichier compose.yaml existant, puis lancez la création.
Sur QNAP avec Container Station, ouvrez Applications → Créer et collez le contenu de qnap-compose.yaml. Copiez également le dossier qnap/ de l’archive sous {dest} avant de démarrer.

"""
    guide += f"""
Depuis un terminal de la machine cible, vous pouvez aussi lancer ces commandes.
  cd {shlex.quote(dest)}
  docker compose config --quiet
  docker compose pull
  docker compose up -d
  docker compose ps

Avec Portainer sur Docker Standalone, ouvrez Stacks → Add stack → Upload. Utilisez le nom {project} et importez compose.yaml. Dans Environment variables, utilisez Load variables from .env file pour charger le .env de cette archive. Cliquez sur Deploy the stack. Les fichiers docker/ doivent déjà se trouver dans le dossier indiqué sur l’hôte du moteur Docker.
"""
    guide += "\n3. Créer le premier compte\n"
    if password:
        guide += f"""Sur un réseau local de confiance, ouvrez http://ADRESSE-DU-NAS:{port}/setup. Remplacez ADRESSE-DU-NAS par son adresse sur votre réseau.
Votre navigateur vous demande d’abord les identifiants temporaires du projet SoundTime.
  Identifiant temporaire : soundtime
  Mot de passe temporaire : {password}
Créez ensuite votre propre compte administrateur SoundTime et terminez les écrans de configuration dans ce navigateur. Choisissez « Instance privée » si la bibliothèque doit être réservée aux personnes connectées.
Une fois SoundTime ouvert, renommez docker/acces/bootstrap.active en bootstrap.termine dans le gestionnaire de fichiers du NAS. Redémarrez ensuite le conteneur nginx du projet depuis l’interface du NAS ou avec « docker compose restart nginx ». Le réseau local utilisera alors directement les comptes SoundTime. Pour un accès par Internet, installez d’abord un reverse proxy HTTPS.
"""
    else:
        guide += f"""L’accès web commence sur la boucle locale du serveur. Sur votre ordinateur, adaptez cette commande à votre compte et à l’adresse de la machine puis lancez-la dans un terminal.
  ssh -N -L 127.0.0.1:{port}:127.0.0.1:{port} votre-compte@votre-serveur
Gardez cette fenêtre ouverte. Visitez http://127.0.0.1:{port}/setup depuis votre ordinateur. Créez votre compte administrateur puis terminez la configuration. Vous pouvez aussi ouvrir la même adresse dans un navigateur sur la machine cible.
"""
        if s["access"] == "lan":
            guide += f"""Pour autoriser ensuite les appareils de votre réseau, réglez SOUNDTIME_BIND_IP dans .env sur l’adresse privée de la machine (par exemple 192.168.1.20). Depuis le dossier du projet, faites « docker compose up -d nginx ». Dans Portainer, modifiez la variable du même nom et redéployez la stack. Les appareils de ce réseau ouvrent http://ADRESSE-PRIVÉE:{port}/. Pour un accès par Internet, gardez la boucle locale et placez un reverse proxy HTTPS sur l’hôte.
"""
    guide += "\n4. Vérifier votre musique\n"
    if s["storage"]["kind"] == "s3":
        guide += ("Le serveur conserve les médias dans le bucket " + s["storage"]["bucket"] +
                  ". Vérifiez que ce bucket existe et que le conteneur peut joindre votre service S3. "
                  "Téléversez un petit morceau puis lancez sa lecture pour vérifier les droits d’écriture et de lecture. "
                  "Administration → Synchronisation nécessite aussi le droit de lister les objets du bucket.\n")
    elif s["musicDirectory"]:
        guide += (f"Votre dossier {s['musicDirectory']} est monté en lecture seule dans /data/music/collection. "
                  "Branchez le disque avant le démarrage et avant chaque synchronisation. "
                  "Ouvrez Administration → Synchronisation pour importer ses titres.\n")
    else:
        guide += "Dans SoundTime, ouvrez Téléverser, choisissez un morceau et lancez sa lecture.\n"
    return guide + """
5. Conserver l’installation
Les volumes Docker conservent le catalogue, les fichiers téléversés sur disque et les données du réseau pair à pair. Conservez les volumes, le dossier du projet et le nom de la stack lors des redémarrages. Sauvegardez la base PostgreSQL, le stockage des médias et les identifiants avant une mise à jour. Pour suspendre les services, utilisez « docker compose stop ». Relancez avec « docker compose up -d ».
En cas de difficulté au démarrage, consultez le journal du projet dans Portainer ou lancez « docker compose logs --tail=100 » dans le dossier du projet.
"""


def archive(s, target, root, storage_env, secret_variables):
    if not isinstance(target, str) or target not in TARGETS:
        raise ValueError("Choisissez une destination parmi NAS, serveur dédié, VPS, VM et cloud.")
    project = "soundtime-" + secrets.token_hex(8)
    guard = target == "nas" and s["access"] == "lan"
    values = {"COMPOSE_PROJECT_NAME": project, "SOUNDTIME_BIND_IP": "127.0.0.1",
              "POSTGRES_PASSWORD": secrets.token_hex(32), "JWT_SECRET": secrets.token_hex(32)}
    env = "".join(f"{key}={value}\n" for key, value in values.items()) + secret_variables
    if s["storage"]["kind"] == "s3":
        values.update(S3_ACCESS_KEY=s["storage"]["accessKey"], S3_SECRET_KEY=s["storage"]["secretKey"])
    model = _model(s, project, storage_env, guard)
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
    files["LISEZ-MOI.txt"] = _instructions(s, target, password, project).encode()
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as packed:
        for name, data in files.items():
            info = zipfile.ZipInfo(name)
            info.create_system = 3
            # The nginx worker reads the password digest with its non-root UID.
            # .env, the cleartext guide and service environment files stay private.
            mode = 0o644 if name == "docker/acces/comptes" else 0o600
            info.external_attr = (0o100000 | mode) << 16
            packed.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED)
    return f"soundtime-{target}.zip", buffer.getvalue()
