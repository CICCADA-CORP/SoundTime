"""Historical French diagnostics understood by current installer clients."""


LEGACY_FRENCH = {
    'Action distante inconnue.':
        'Unknown remote action.',
    'Activation de l’accès depuis votre réseau.':
        'Enabling access from your network.',
    'Ce chemin désigne un fichier. Choisissez un dossier.':
        'This path points to a file. Choose a folder.',
    'Ce dossier contient déjà des fichiers. Choisissez un dossier vide ou une installation créée avec cet assistant.':
        'This folder already contains files. Choose an empty folder or an installation created with this installer.',
    'Choisissez le chemin complet de votre dossier de musique.':
        'Enter the full path to your music folder.',
    'Choisissez le dossier de musique sur disque ou le bucket S3 pour cette installation.':
        'Choose either a music folder on disk or an S3 bucket for this installation.',
    'Choisissez le stockage sur disque ou renseignez les paramètres S3.':
        'Choose disk storage or enter your S3 settings.',
    'Choisissez un chemin de dossier complet.':
        'Enter a full folder path.',
    'Choisissez un dossier dans lequel votre compte peut écrire.':
        'Choose a folder your account can write to.',
    'Choisissez un dossier de musique dédié, séparé du dossier d’installation.':
        'Choose a music folder separate from the installation folder.',
    'Choisissez un dossier d’installation dédié avec un chemin absolu.':
        'Choose a dedicated installation folder and use its absolute path.',
    'Choisissez un dossier d’installation dédié, sous la racine du disque.':
        'Choose an installation folder below the root of the drive.',
    'Choisissez un dossier d’installation dédié.':
        'Choose a dedicated installation folder.',
    'Choisissez un hôte Linux avec Docker Engine et Compose.':
        'Choose a Linux host with Docker Engine and Compose.',
    'Choisissez un numéro de port entre 1 et 65535.':
        'Choose a port between 1 and 65535.',
    'Choisissez une destination et un compte SSH.':
        'Choose a destination and an SSH account.',
    'Choisissez une destination parmi NAS, serveur dédié, VPS, VM et cloud.':
        'Choose NAS, dedicated server, VPS, virtual machine or cloud as the destination.',
    'Choisissez une installation existante configurée pour le réseau local.':
        'Select an existing installation configured for local network access.',
    'Choisissez une option d’accès et de partage proposée par l’assistant.':
        'Choose one of the access and sharing options offered by the installer.',
    'Configuration S3':
        'S3 configuration',
    'Connexion SSH interrompue. Vérifiez l’adresse, le port, la clé et l’accès réseau au serveur.':
        'The SSH connection was interrupted. Check the server address, port, key and network connection.',
    'Connexion au serveur':
        'Server connection',
    'Connexion à {host} avec votre clé SSH.':
        'Connecting to {host} with your SSH key.',
    'Docker Compose n’a trouvé aucune image dans la configuration.':
        'Docker Compose found no images in the configuration.',
    'Docker est démarré. Compose {version} est disponible.':
        'Docker is running. Compose {version} is available.',
    'Docker est installé. Démarrez-le et vérifiez que votre compte y a accès, puis réessayez.':
        'Docker is installed. Start it and make sure your account can access it, then try again.',
    'Docker et Compose':
        'Docker and Compose',
    'Docker met trop de temps à répondre. Vérifiez son état et votre connexion, puis réessayez.':
        'Docker took too long to respond. Check Docker and your connection, then try again.',
    'Docker n’a pas pu démarrer les services. Vérifiez l’espace disque et le partage du dossier dans Docker Desktop, puis réessayez. Le journal « docker compose logs --tail=100 » est disponible dans ce dossier.':
        'Docker could not start the services. Check disk space and Docker Desktop file sharing, then try again. Run “docker compose logs --tail=100” in this folder for details.',
    'Docker n’a pas pu télécharger {image}. Vérifiez votre connexion, l’espace disque et l’accès au registre, puis réessayez.':
        'Docker could not download {image}. Check your connection, disk space and registry access, then try again.',
    'Dossier d’installation':
        'Installation folder',
    'Démarrage de la bibliothèque et de ses services.':
        'Starting the music library and its services.',
    'Entrez un nom d’hôte ou une adresse IPv4, par exemple localhost ou musique.maison.':
        'Enter a host name or IPv4 address, such as localhost or music.home.',
    'Extrayez l’archive universelle complète pour utiliser le moteur de la machine distante.':
        'Extract the complete universal archive to use the deployment engine for the remote host.',
    'Impossible de préparer la protection du premier compte avec cette configuration Nginx.':
        'This Nginx configuration cannot protect the first account during setup.',
    'Impossible de préparer le moteur sur le serveur. Vérifiez l’espace disponible et les droits du compte.':
        'Could not prepare the deployment engine on the server. Check the free space and account permissions.',
    'Impossible de reprendre automatiquement ces réglages. Restaurez la configuration de cette installation ou choisissez un dossier vide.':
        'These settings could not be recovered automatically. Restore the original configuration or choose an empty folder.',
    'Impossible d’identifier le serveur. Utilisez Linux, macOS ou Windows avec OpenSSH.':
        'Could not identify the server. Use Linux, macOS or Windows with OpenSSH.',
    'Indiquez la région de votre bucket, par exemple eu-west-3 pour AWS ou auto pour R2.':
        'Enter the bucket region, such as eu-west-3 for AWS or auto for R2.',
    'Indiquez le nom du bucket existant (3 à 63 lettres minuscules, chiffres, points ou tirets).':
        'Enter the existing bucket name using 3 to 63 lowercase letters, digits, dots or hyphens.',
    'Indiquez le nom du compte Linux utilisé pour la connexion SSH.':
        'Enter the Linux username for the SSH connection.',
    'Indiquez le nom du compte utilisé pour la connexion SSH.':
        'Enter the username for the SSH connection.',
    'Indiquez le nom d’hôte ou l’adresse IP de la machine.':
        'Enter the machine’s host name or IP address.',
    'Indiquez un nom d’hôte ou une adresse IP, sans protocole ni chemin.':
        'Enter a host name or IP address without a protocol or path.',
    'Indiquez un port SSH entre 1 et 65535.':
        'Enter an SSH port between 1 and 65535.',
    'Indiquez un préfixe S3 sans composant « .. ».':
        'Enter an S3 prefix without a “..” path component.',
    'Installez Docker Desktop sur Mac ou Windows, ou Docker Engine avec Compose sur Linux, puis relancez la vérification.':
        'Install Docker Desktop on Mac or Windows, or Docker Engine with Compose on Linux, then run the checks again.',
    'Installez Python 3.9 ou ultérieur sur le serveur ou utilisez le projet Docker à importer.':
        'Install Python 3.9 or later on the server, or export a Docker project to import there.',
    'Installez le client OpenSSH sur cet ordinateur, ou choisissez le projet Docker à importer.':
        'Install the OpenSSH client on this computer, or export a Docker project to import on the target.',
    'La clé SSH est reconnue et l’agent fonctionne sur {host}.':
        'SSH authenticated successfully and the agent is running on {host}.',
    'La clé de cet hôte n’est pas reconnue. Vérifiez son empreinte auprès de votre hébergeur, puis connectez-vous une première fois avec SSH depuis cet ordinateur.':
        'This host key is unknown. Verify its fingerprint with your provider, then connect once using SSH from this computer.',
    'La configuration Docker est invalide. Vérifiez votre version de Compose.':
        'The Docker configuration is invalid. Check your Compose version.',
    'Le client SSH n’a pas pu démarrer.':
        'The SSH client could not start.',
    'Le disque utilisé par Docker est plein. Libérez de la place, puis réessayez avec le même dossier.':
        'Docker’s disk is full. Free some space and try again using the same folder.',
    'Le dossier de cette installation a été déplacé ou ses identifiants sont incomplets. Restaurez la configuration d’origine ou choisissez un dossier vide.':
        'The installation folder was moved or its credentials are incomplete. Restore the original configuration or choose an empty folder.',
    'Le dossier de musique doit exister et être lisible. Vérifiez son chemin ou reconnectez le disque.':
        'The music folder must exist and be readable. Check its path or reconnect the drive.',
    'Le dossier est disponible pour SoundTime.':
        'The folder is available for SoundTime.',
    'Le fichier {name} a été modifié. Conservez ce dossier et utilisez Docker Compose pour le gérer, ou choisissez un dossier vide.':
        '{name} has changed. Keep this folder and manage it with Docker Compose, or choose an empty folder.',
    'Le fichier {name} a été modifié. Gérez cette installation depuis Docker Compose ou choisissez un dossier vide.':
        '{name} has changed. Manage this installation with Docker Compose or select an empty folder.',
    'Le moteur est incompatible avec ce système. Sur un NAS, utilisez le projet Docker à importer.':
        'The installer engine is incompatible with this system. On a NAS, use the Docker project import option.',
    'Le port {port} est occupé ou réservé. Choisissez un autre numéro dans les réglages.':
        'Port {port} is in use or reserved. Choose another port in the settings.',
    'Le serveur a mis trop de temps à répondre. Vérifiez la connexion SSH.':
        'The server timed out. Check the SSH connection.',
    'Le serveur a refusé le compte ou sa clé SSH. Vérifiez le nom du compte et chargez sa clé dans votre agent SSH.':
        'The server rejected the account or its SSH key. Check the username and load the key into your SSH agent.',
    'Le serveur n’a pas pu accéder au dossier ou à Docker. Vérifiez les droits du compte SSH.':
        'The server could not access the folder or Docker. Check the SSH account permissions.',
    'Le système détecté ne correspond pas à la destination choisie.':
        'The detected operating system does not match the selected destination.',
    'Le tunnel SSH ne rejoint pas SoundTime. Vérifiez le port web et la redirection TCP du serveur.':
        'The SSH tunnel cannot reach SoundTime. Check the web port and TCP forwarding on the server.',
    'Les champs S3 doivent tenir chacun sur une ligne de texte.':
        'Enter each S3 setting on a single line.',
    'Les fichiers Docker de l’installation doivent se trouver dans le dossier choisi.':
        'The Docker files must be inside the selected installation folder.',
    'Les ports choisis sont disponibles pour cette installation.':
        'The selected ports are available.',
    'Les réglages sont incomplets. Relancez l’installateur et réessayez.':
        'Some settings are missing. Reopen the installer and try again.',
    'L’adresse IPv6 du serveur est invalide.':
        'The server’s IPv6 address is invalid.',
    'L’adresse S3 doit être une URL HTTP ou HTTPS sans identifiants, paramètres ou fragment. Laissez-la vide pour AWS.':
        'Enter an HTTP or HTTPS endpoint without credentials, query parameters or a fragment. Leave it blank for AWS.',
    'L’agent SSH est absent de ce paquet. Rouvrez une distribution complète de l’assistant.':
        'This package is missing the SSH agent. Open a complete copy of the installer.',
    'L’agent distant a rencontré une erreur. Relancez la vérification.':
        'The remote agent encountered an error. Run the checks again.',
    'L’installateur prend en charge les processeurs 64 bits Intel/AMD et ARM.':
        'The installer supports 64-bit Intel/AMD and ARM processors.',
    'L’installation existante sera reprise.':
        'The existing installation will resume.',
    'L’installation ou ses réglages ont changé. Relancez la vérification avec le même dossier.':
        'The installation or its settings have changed. Run the checks again using the same folder.',
    'Mettez à jour Docker Compose vers la version 2.24.4 ou ultérieure, puis réessayez.':
        'Update Docker Compose to version 2.24.4 or later, then try again.',
    'Ouvrez Docker Desktop ou démarrez Docker Engine, puis relancez la vérification.':
        'Open Docker Desktop or start Docker Engine, then run the checks again.',
    'Ouvrez un terminal pour confirmer l’empreinte SSH du serveur.':
        'Open a terminal to confirm the server’s SSH fingerprint.',
    'Paramètres renseignés. Un premier téléversement et sa lecture permettront de vérifier les droits sur le bucket.':
        'The S3 settings are filled in. Upload and play a track to verify access to the bucket.',
    'Ports réseau':
        'Network ports',
    'Pour exporter une configuration S3, fournissez les clés du bucket.':
        'Enter the bucket credentials to export an S3 configuration.',
    'Pour modifier le stockage S3, renseignez de nouveau les clés du bucket.':
        'To change the S3 storage settings, enter the bucket credentials again.',
    'Python 3.9 ou plus récent est requis sur le serveur.':
        'Python 3.9 or later is required on the server.',
    'Renseignez la clé d’accès et la clé secrète du service S3.':
        'Enter the access key and secret key for your S3 service.',
    'Restaurez le fichier {name} de l’installation d’origine avant de reprendre.':
        'Restore {name} from the original installation before continuing.',
    'Réglages':
        'Settings',
    'SoundTime démarre encore ou un service rencontre un problème. Réessayez avec le même dossier. Le journal « docker compose logs --tail=100 » dans ce dossier indique le service concerné.':
        'SoundTime is still starting or a service needs attention. Retry with the same folder. Run “docker compose logs --tail=100” in that folder to see which service is affected.',
    'SoundTime fonctionne sur le serveur. Vérifiez le client SSH et la redirection TCP, puis ouvrez le tunnel indiqué pour accéder à votre bibliothèque.':
        'SoundTime is running on the server. Check your SSH client and TCP forwarding, then open the indicated tunnel to access your library.',
    'SoundTime utilise des conteneurs Linux x86-64 ou ARM64. Sur Windows, activez les conteneurs Linux dans Docker Desktop.':
        'SoundTime runs in Linux x86-64 or ARM64 containers. On Windows, switch Docker Desktop to Linux containers.',
    'Sélectionnez le moteur Docker de cet ordinateur. Pour un serveur distant, ouvrez l’assistant sur ce serveur.':
        'Select this computer’s Docker engine. To install on a remote host, use the remote installation option or run the installer there.',
    'Terminez la création du compte et la configuration dans SoundTime, puis activez à nouveau l’accès réseau.':
        'Finish creating your account and setting up SoundTime, then enable network access again.',
    'Téléchargement des images manquantes. Docker réutilise les images déjà disponibles.':
        'Downloading missing images. Docker will reuse images already available.',
    'Un fichier de l’installation est absent ou remplacé. Vérifiez le dossier choisi.':
        'An installation file is missing or has been replaced. Check the selected folder.',
    'Un port vient d’être réservé par une autre application. Changez le port dans les réglages et relancez l’installation.':
        'Another application just reserved the port. Change the port in settings and restart the installation.',
    'Une autre installation utilise déjà ce dossier. Attendez sa fin, puis réessayez.':
        'Another installation is using this folder. Wait for it to finish, then try again.',
    'Une variable du projet contient un retour à la ligne.':
        'A project variable contains a line break.',
    'Utilisez une adresse IPv6 sans identifiant d’interface.':
        'Use an IPv6 address without an interface identifier.',
    'Variable du projet QNAP manquante : {name}':
        'Missing QNAP project variable {name}.',
    'Vérification de Docker, du dossier et des ports.':
        'Checking Docker, the folder and the ports.',
    'Vérification de l’accès à SoundTime.':
        'Checking access to SoundTime.',
    'Vérification du serveur, de la base de données et de la page de configuration.':
        'Checking the server, database and setup page.',
    '{i}/{total} · Téléchargement de {image}.':
        '{i}/{total} · Downloading {image}.',
    '{i}/{total} · {image} est disponible sur cet ordinateur.':
        '{i}/{total} · {image} is already available on this computer.',
    "Les réglages sont incomplets. Rechargez la page de l’assistant.":
        "Some settings are missing. Reopen the installer and try again.",
}
