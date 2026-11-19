#!/bin/sh
cd "$(dirname "$0")" || exit 1
if ! python3 -c 'import sys; sys.exit(sys.version_info < (3, 9))' >/dev/null 2>&1; then
    printf '%s\n' 'Cette copie des sources nécessite Python 3.9 ou plus récent.' \
        'Vous pouvez aussi utiliser l’application SoundTime Installer des distributions du projet.' \
        'Appuyez sur Entrée pour quitter.'
    read -r ignored
    exit 1
fi
exec python3 launcher.py "$@"
