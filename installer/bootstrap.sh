#!/bin/sh
# Download the pinned portable release and run its wizard on the caller's terminal.
set -eu
umask 077

url=https://github.com/CICCADA-CORP/SoundTime/releases/download/installer-v2.0.0/SoundTime-CLI.zip
expected=169dff5a9fd6b3e4cb64a3644c9ce86f4f070fed36026d2cf869d16c18cfc0bf

message() {
    case ${SOUNDTIME_LANG:-${LC_ALL:-${LC_MESSAGES:-${LANG:-en}}}} in
        fr*) printf '%s\n' "$2" ;;
        es*) printf '%s\n' "$3" ;;
        ru*) printf '%s\n' "$4" ;;
        zh*) printf '%s\n' "$5" ;;
        *)   printf '%s\n' "$1" ;;
    esac >&2
}

if command -v unzip >/dev/null 2>&1; then
    extractor=unzip
elif command -v bsdtar >/dev/null 2>&1; then
    extractor=bsdtar
else
    message 'Install unzip or bsdtar and run the command again.' \
        'Installez unzip ou bsdtar, puis relancez la commande.' \
        'Instala unzip o bsdtar y vuelve a ejecutar el comando.' \
        'Установите unzip или bsdtar и повторите команду.' \
        '请安装 unzip 或 bsdtar，然后重新运行命令。'
    exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum; }
elif command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256; }
else
    message 'Install sha256sum or shasum and run the command again.' \
        'Installez sha256sum ou shasum, puis relancez la commande.' \
        'Instala sha256sum o shasum y vuelve a ejecutar el comando.' \
        'Установите sha256sum или shasum и повторите команду.' \
        '请安装 sha256sum 或 shasum，然后重新运行命令。'
    exit 1
fi

scratch=$(mktemp -d "${TMPDIR:-/tmp}/soundtime-cli.XXXXXXXX")
trap 'rm -rf -- "$scratch"' 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
archive=$scratch/SoundTime-CLI.zip

message 'Downloading SoundTime CLI 2.0.0…' 'Téléchargement de SoundTime CLI 2.0.0…' \
    'Descargando SoundTime CLI 2.0.0…' 'Загрузка SoundTime CLI 2.0.0…' '正在下载 SoundTime CLI 2.0.0…'
curl --fail --location --silent --show-error --retry 2 --proto '=https' --proto-redir '=https' --output "$archive" "$url"
digest=$(sha256 < "$archive")
if [ "${digest%% *}" != "$expected" ]; then
    message 'SHA-256 verification failed. Download SoundTime again.' \
        'La vérification SHA-256 a échoué. Téléchargez à nouveau SoundTime.' \
        'Falló la verificación SHA-256. Vuelve a descargar SoundTime.' \
        'Ошибка проверки SHA-256. Загрузите SoundTime ещё раз.' \
        'SHA-256 校验失败。请重新下载 SoundTime。'
    exit 1
fi
if [ "$extractor" = unzip ]; then
    unzip -q "$archive" -d "$scratch"
else
    bsdtar -xf "$archive" -C "$scratch"
fi
sh "$scratch/soundtime" "$@"
