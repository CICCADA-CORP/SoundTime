#!/bin/sh
# Download the pinned portable release and run its wizard on the caller's terminal.
set -eu
umask 077

url=https://github.com/CICCADA-CORP/SoundTime/releases/download/installer-v2.0.0/SoundTime-CLI.zip
expected=169dff5a9fd6b3e4cb64a3644c9ce86f4f070fed36026d2cf869d16c18cfc0bf

if command -v unzip >/dev/null 2>&1; then
    extractor=unzip
elif command -v bsdtar >/dev/null 2>&1; then
    extractor=bsdtar
else
    printf '%s\n' 'Install unzip or bsdtar and run the command again.' >&2
    exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum; }
elif command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256; }
else
    printf '%s\n' 'Install sha256sum or shasum and run the command again.' >&2
    exit 1
fi

scratch=$(mktemp -d "${TMPDIR:-/tmp}/soundtime-cli.XXXXXXXX")
trap 'rm -rf -- "$scratch"' 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
archive=$scratch/SoundTime-CLI.zip

printf '%s\n' 'Downloading SoundTime CLI...' >&2
curl --fail --location --silent --show-error --retry 2 --proto '=https' --proto-redir '=https' --output "$archive" "$url"
digest=$(sha256 < "$archive")
if [ "${digest%% *}" != "$expected" ]; then
    printf '%s\n' 'SHA-256 verification failed. Download SoundTime again.' >&2
    exit 1
fi
if [ "$extractor" = unzip ]; then
    unzip -q "$archive" -d "$scratch"
else
    bsdtar -xf "$archive" -C "$scratch"
fi
sh "$scratch/soundtime" "$@"
