# Installer SoundTime depuis le terminal

La machine qui lance la commande peut être différente de celle qui gardera votre musique. Téléchargez l’archive **SoundTime-CLI.zip**, extrayez-la entièrement et conservez le dossier `bin` à côté des lanceurs. Elle contient les exécutables pour Windows, macOS et Linux sur processeurs Intel/AMD 64 bits et ARM64. Le lancement de la version distribuée et le transfert par SSH utilisent le moteur inclus. [Télécharger la CLI](../README.md#guided-installation) · [Installer manuellement avec Docker Compose](../README.md#manual-installation)

## Lancer l’installation

Ouvrez un terminal dans le dossier extrait et entrez la commande de votre système.

| Terminal | Commande |
| --- | --- |
| macOS ou Linux | `./soundtime` |
| PowerShell sous Windows | `.\soundtime.cmd` |
| Invite de commandes Windows (cmd) | `soundtime.cmd` |

La CLI choisit la langue du terminal. Pour en choisir une autre, ajoutez `--lang fr`, `en`, `es`, `ru` ou `zh`. Par exemple, `.\soundtime.cmd --lang fr` démarre l’installation en français depuis PowerShell. Utilisez les numéros du menu et appuyez sur Entrée pour accepter une valeur proposée.

## Installer sur cet ordinateur

1. Choisissez **Cet ordinateur**. Le terminal affiche le système détecté et propose les réglages de la machine.
2. Choisissez **Réglages recommandés** pour une instance privée sur cet ordinateur ou **Personnaliser** pour choisir les dossiers, le port, le partage sur le réseau ou un stockage S3.
3. Lisez le récapitulatif. La CLI vérifie le dossier cible, les ports et Docker avant de démarrer les services. Sur Mac ou Windows, lancez Docker Desktop avec les conteneurs Linux. Sous Linux, utilisez Docker Engine avec le module Compose. Le déploiement piloté demande Compose 2.24.4 ou ultérieur. [Docker Desktop](https://docs.docker.com/desktop/) · [Docker Engine et Compose sous Linux](https://docs.docker.com/compose/install/linux/)
4. Confirmez l’installation, puis créez le compte administrateur quand le terminal le propose. Votre mot de passe est masqué à la saisie. Si vous avez choisi le réseau local, la CLI propose d’activer cet accès une fois le compte créé.
5. Conservez l’adresse affichée pour écouter votre bibliothèque. SoundTime reste en service dans Docker après la fermeture du terminal.

## Installer sur une autre machine par SSH

Utilisez ce parcours pour un autre Mac ou PC, un serveur Linux, un VPS, une VM ou une instance cloud. Le système distant doit disposer de Docker et de stockage persistant. Docker Desktop doit être démarré si c’est lui qui héberge les conteneurs. Pour Linux, les exécutables distribués ciblent une distribution avec glibc 2.35 ou ultérieure. Pour un NAS avec son propre gestionnaire de conteneurs, suivez le parcours d’export ci-dessous.

1. Activez SSH sur la machine cible et préparez un compte autorisé à y utiliser Docker. Une clé SSH utilisable depuis votre terminal permet de piloter les étapes successives. Pour une clé protégée, chargez-la dans votre agent SSH. Lors du premier contact, comparez l’empreinte présentée par OpenSSH à celle de votre administrateur ou de votre hébergeur.
2. Choisissez **Une autre machine**, puis **Installer via SSH**. Indiquez le nom d’hôte ou l’adresse IP, le nom du compte et le port SSH (généralement 22). La CLI identifie le système et le processeur de l’hôte puis transfère l’exécutable correspondant dans un dossier temporaire. Les chemins de bibliothèque et d’installation que vous saisissez concernent cet hôte.
3. Confirmez les réglages, puis terminez la création du compte dans le terminal. L’accès privé au serveur passe par un tunnel SSH. La CLI affiche la commande permettant de le rouvrir après la fermeture de l’installateur.
4. Pour donner accès à d’autres personnes depuis Internet, configurez HTTPS et le pare-feu du serveur ou du fournisseur cloud. [Déploiement avec HTTPS](deployment.md)

## Installer sur un NAS

Vérifiez que votre modèle possède un processeur x86-64 ou ARM64 et propose Container Manager ou Container Station. Dans la CLI, choisissez une autre machine puis le projet à importer sur un NAS. Sélectionnez votre gestionnaire Synology, QNAP ou autre pour adapter le chemin suggéré. Indiquez un dossier de projet dédié **sur le NAS**. Pour écouter sur le réseau local, sélectionnez l’accès réseau. Enregistrez le ZIP créé par la CLI puis extrayez tous ses fichiers dans ce dossier, y compris le fichier caché `.env`, le dossier `docker` et `LISEZ-MOI.txt`. Ce ZIP contient les identifiants de l’installation. Gardez-le dans un partage privé.

### Synology avec Container Manager

1. Dans **File Station**, créez par exemple `/volume1/docker/soundtime` et placez-y le projet extrait. Au même niveau que `compose.yaml` doivent se trouver `.env` et le dossier `docker/` contenant `nginx.conf`. Respectez le chemin du dossier indiqué lors de l’export, car le projet NAS monte ces fichiers depuis leur emplacement sur l’hôte.
2. Dans **Container Manager → Projet → Créer**, donnez un nom au projet et choisissez ce même dossier comme chemin de travail. Pour la source YAML, utilisez le contenu de `compose.yaml` ou fournissez une copie nommée `docker-compose.yml` si le sélecteur demande ce nom. Le portail Web Station est facultatif. Terminez l’assistant DSM et démarrez le projet. [Projets dans Synology Container Manager](https://kb.synology.com/en-global/DSM/help/ContainerManager/docker_project)
3. Si la version du gestionnaire refuse la configuration, utilisez la procédure Compose en ligne de commande ci-dessous depuis **ce même dossier**. Elle indique explicitement à Docker où lire `.env`. [Variables de projet dans Docker Compose](https://docs.docker.com/compose/how-tos/environment-variables/variable-interpolation/)

### QNAP avec Container Station 3

1. Déposez le projet extrait dans le dossier **exact** indiqué lors de l’export, par exemple `/share/Container/soundtime`. Conservez les sous-dossiers `docker/` et `qnap/`. L’installation utilise des chemins absolus vers les fichiers du NAS. Le dossier privé `qnap/` fournit les paramètres du serveur et de PostgreSQL.
2. Ouvrez `qnap-compose.yaml` dans un éditeur de texte. Dans **Container Station → Applications → Créer**, nommez l’application, collez **tout le contenu de ce fichier**, choisissez **Valider**, puis créez l’application. Conservez le même nom lorsque vous la gérez par la suite. Le fichier à coller contient les chemins vers les paramètres protégés. [Applications dans QNAP Container Station 3](https://www.qnap.com/en-us/how-to/tutorial/article/how-to-use-container-station-3)
3. Si la validation signale un fichier absent ou un accès refusé, vérifiez le chemin du partage et les droits de lecture de Container Station sur les sous-dossiers du projet. Pour une version du gestionnaire dont le service ne peut pas lire ces fichiers sur l’hôte, utilisez le projet standard depuis son dossier avec Compose.

### Démarrer le projet extrait avec Compose

Connectez-vous à l’hôte et placez-vous dans son dossier de projet. Utilisez un compte autorisé à exécuter Docker sur cet hôte. La vérification silencieuse lit les paramètres sans afficher de secrets.

```sh
docker compose --env-file .env -f compose.yaml config --quiet
docker compose --env-file .env -f compose.yaml up -d
docker compose --env-file .env -f compose.yaml ps
```

Le téléchargement et la préparation de la base peuvent prendre quelques minutes. Les instructions personnalisées et le port choisi se trouvent dans la notice du projet (`LISEZ-MOI.txt` en français). Si le port est occupé, exportez la configuration avec un autre port avant de créer des comptes ou d’importer de la musique. Pour une installation existante, conservez son dossier, son projet, ses identifiants et ses volumes.

### Créer le premier compte sur le NAS

Avec l’option d’accès réseau sur le NAS, ouvrez `http://ADRESSE-DU-NAS:8880/setup` depuis votre réseau de confiance. Adaptez le port si vous l’avez personnalisé. Le navigateur demande d’abord l’identifiant temporaire `soundtime` et le mot de passe qui figure dans la notice du projet (`LISEZ-MOI.txt` en français). Créez ensuite votre **propre compte administrateur SoundTime** et terminez la configuration. Vérifiez que vous pouvez vous connecter à votre catalogue.

Dans le dossier du projet, renommez `docker/acces/bootstrap.active` en `bootstrap.termine` en le laissant dans `docker/acces/`, puis redémarrez le service `nginx`. Dans un terminal, utilisez `docker compose --env-file .env -f compose.yaml restart nginx`. Les utilisateurs pourront alors accéder à SoundTime avec les règles de confidentialité que vous avez choisies. Gardez l’accès sur votre réseau privé tant que HTTPS et les règles de pare-feu pour une publication Internet ne sont pas configurés.

## Choisir le stockage audio

### Dossier d’albums existant

En mode personnalisé, indiquez le dossier qui contient vos albums. SoundTime y accède en lecture seule. Après avoir créé votre compte, ouvrez **Administration → Synchronisation → Lancer la synchronisation** pour retrouver vos morceaux dans le catalogue. Les fichiers de cette collection restent sur leur support d’origine.

Si la collection se trouve sur un disque externe, branchez-le avant chaque démarrage de SoundTime et gardez-le connecté pendant la synchronisation et l’écoute. Si Docker Desktop signale un refus de montage, autorisez le dossier ou le disque dans ses réglages de partage de fichiers, puis relancez la vérification.

Pour une collection sur le NAS, indiquez le chemin du partage tel que le voit l’hôte Docker, par exemple `/volume1/music` ou `/share/Musique`. Une lettre de lecteur Windows sur votre poste ne désigne pas un dossier du NAS. Si le serveur Linux lit la musique depuis un partage SMB ou NFS, montez d’abord ce partage sur le serveur ou suivez le guide des bibliothèques réseau. [Bibliothèques SMB et NFS](deployment.md#read-only-smb-and-nfs-music-libraries)

### Amazon S3 ou service compatible

Choisissez S3 pour stocker les fichiers audio gérés par SoundTime dans un bucket privé existant. Le serveur, la base de données et leurs volumes restent sur la machine d’hébergement. Pour une nouvelle installation, un bucket ou un préfixe dédié permet de sauvegarder et de retrouver facilement les objets de SoundTime.

| Réglage | Valeur à saisir |
| --- | --- |
| Bucket | Son nom exact, par exemple `ma-musique-privee`. |
| Région | La région du bucket AWS, par exemple `eu-west-3`, ou la région indiquée par le service compatible. |
| Endpoint | En mode Amazon S3, l’application sélectionne l’adresse AWS à partir de la région. Pour un service compatible, saisissez l’URL de l’API indiquée par le fournisseur et joignable depuis le conteneur SoundTime. |
| Préfixe | Une valeur facultative comme `soundtime` pour ranger la musique sous `soundtime/`. |
| Clé d’accès et clé secrète | Une paire de clés dédiée disposant des droits sur le bucket et le préfixe choisis. |

SoundTime utilise `s3:ListBucket` pour lister les fichiers ainsi que `s3:GetObject`, `s3:PutObject` et `s3:DeleteObject` pour lire, ajouter et supprimer ses objets. Limitez la liste au préfixe voulu et les actions sur objets à son contenu. Des clés KMS gérées par votre organisation peuvent exiger des permissions supplémentaires. [Permissions S3 par opération](https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-with-s3-policy-actions.html) · [Endpoints par région AWS](https://docs.aws.amazon.com/general/latest/gr/s3.html) · [Préfixes S3](https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-prefixes.html)

La vérification de l’assistant contrôle les champs S3. Après l’installation, importez un petit fichier audio et lisez-le dans SoundTime pour confirmer l’écriture et la lecture réelles sur le bucket. Gardez le bucket, le préfixe et les clés associés au projet. Les exports Compose qui utilisent S3 contiennent aussi les identifiants de stockage.

## Commandes pour les serveurs et l’automatisation

Dans un terminal Windows, remplacez `./soundtime` par `.\soundtime.cmd` (PowerShell) ou `soundtime.cmd` (cmd). Ajoutez `--help` après une commande pour lire ses options.

```sh
./soundtime --lang fr check
./soundtime --lang fr install --directory "/srv/soundtime" --port 8880
./soundtime --lang fr setup --port 8880 --admin-user admin --admin-email admin@example.org
./soundtime --lang fr check --target auto --ssh-host music.example.org --ssh-user deploy
./soundtime --lang fr export --target nas --directory "/volume1/docker/soundtime" --output soundtime-nas.zip
```

Le mode `--json` retourne le résultat sur la sortie standard et réserve la sortie d’erreur à la progression. Un code de retour nul indique que la commande a réussi. Le mode interactif demande les secrets sans les afficher. Pour l’automatisation, injectez `SOUNDTIME_ADMIN_PASSWORD` pour le premier compte ou `SOUNDTIME_S3_ACCESS_KEY` et `SOUNDTIME_S3_SECRET_KEY` pour le stockage S3 depuis votre gestionnaire de secrets.

## Si une vérification échoue

| Message ou situation | Action |
| --- | --- |
| Docker indisponible ou mauvais type de conteneur | Démarrez le moteur sur la machine de destination. Docker Desktop pour Windows doit utiliser les conteneurs Linux. Relancez `check`. |
| Échec de connexion SSH | Vérifiez le nom du compte, le port et l’empreinte du serveur. Essayez `ssh -p 22 utilisateur@hote` depuis le même terminal et vérifiez que votre clé est utilisable. |
| Port occupé ou dossier inaccessible | Choisissez un autre port ou un dossier accessible au compte qui exécute Docker. Pour une reprise, conservez le dossier du projet existant. |
| Le lanceur ne trouve pas le moteur | Extrayez à nouveau toute l’archive dans le même dossier, y compris les six sous-dossiers de `bin`. |
| Le système signale un éditeur inconnu | Utilisez la release officielle et son empreinte SHA-256. Les exécutables Windows n’ont pas de certificat d’éditeur et les exécutables macOS ne sont pas notariés. [Assistance Apple](https://support.apple.com/fr-fr/102445) |

Le dossier d’installation contient la configuration et le fichier confidentiel `.env`. Les comptes et le catalogue se trouvent dans les volumes Docker du projet. Pour sauvegarder le serveur, conservez la configuration, les volumes, le bucket S3 éventuel et les originaux de la collection externe. [Sauvegardes](deployment.md#backups)
