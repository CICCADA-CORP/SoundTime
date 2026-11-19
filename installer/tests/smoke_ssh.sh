#!/usr/bin/env bash
# Exercise the shipped Linux CLI over an isolated loopback sshd on a CI runner.
set -euo pipefail
tmp=$(mktemp -d "$HOME/.soundtime-ssh-test.XXXXXXXX")
port=$(python -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')
login=$(id -un)
finish() {
    if test -s "$tmp/sshd.pid"; then sudo kill "$(cat "$tmp/sshd.pid")" || true; fi
    rm -rf -- "$tmp"
}
trap finish EXIT
ssh-keygen -q -t ed25519 -N '' -f "$tmp/host"
ssh-keygen -q -t ed25519 -N '' -f "$tmp/client"
printf '[127.0.0.1]:%s %s\n' "$port" "$(cat "$tmp/host.pub")" > "$tmp/known_hosts"
cat > "$tmp/sshd.conf" <<EOF
Port $port
ListenAddress 127.0.0.1
HostKey $tmp/host
PidFile $tmp/sshd.pid
AuthorizedKeysFile $tmp/client.pub
AllowUsers $login
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM yes
EOF
cat > "$tmp/client.conf" <<EOF
Host 127.0.0.1
    IdentityFile $tmp/client
    IdentitiesOnly yes
    UserKnownHostsFile $tmp/known_hosts
EOF
sudo mkdir -p /run/sshd
sudo /usr/sbin/sshd -f "$tmp/sshd.conf"
mkdir "$tmp/tools"
cat > "$tmp/tools/ssh" <<'EOF'
#!/bin/sh
exec /usr/bin/ssh -F "$SOUNDTIME_TEST_SSH_CONFIG" "$@"
EOF
chmod 700 "$tmp/tools/ssh"
export SOUNDTIME_TEST_SSH_CONFIG="$tmp/client.conf"
export SOUNDTIME_TEST_SSH_PORT="$port" SOUNDTIME_TEST_SSH_USER="$login"
export SOUNDTIME_TEST_CLI="$PWD/dist/installer/bin/linux-x64/soundtime"
export PATH="$tmp/tools:$PATH"
python installer/tests/smoke_docker.py
