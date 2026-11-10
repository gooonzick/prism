#!/usr/bin/env bash
# Starts a Linux machine for the end-to-end tests: a container running sshd, with a throwaway
# key authorized for the user `me` and the CLI-only Linux build of prism in ~/.local/bin: the
# static musl build releases ship. Needs Docker (on macOS, Colima or Docker Desktop running).
# Everything lives in ~/.cache/prism-e2e (the container runtime may not see /var/folders);
# your own ~/.ssh is never read or written. Prints the ssh target, an alias in the generated
# ssh_config there.
#
#   ./scripts/e2e-ssh.sh && cargo test -- --ignored e2e_
#   ./scripts/e2e-ssh.sh --down      # stop the container and delete ~/.cache/prism-e2e
set -euo pipefail
cd "$(dirname "$0")/.."

CACHE="$HOME/.cache/prism-e2e"
NAME="prism-e2e"
IMAGE="prism-e2e-sshd"

if [[ "${1:-}" == "--down" ]]; then
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  rm -rf "$CACHE"
  echo "Stopped $NAME and removed $CACHE"
  exit 0
fi

if ! docker info >/dev/null 2>&1; then
  echo "Docker isn't answering: start it (on macOS: colima start, or Docker Desktop) and run this again" >&2
  exit 1
fi

mkdir -p "$CACHE/target" "$CACHE/registry" "$CACHE/git"

# The CLI-only build for the container's Linux, as scripts/package-cli.sh makes it for a
# release: static musl, with the toolchain rust-toolchain.toml pins.
TOOLCHAIN="$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)"
docker build -q -t "$IMAGE-build" - >/dev/null <<DOCKERFILE
FROM rust:$TOOLCHAIN-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends musl-tools && rm -rf /var/lib/apt/lists/* \
 && rustup target add "\$(uname -m)-unknown-linux-musl"
DOCKERFILE
docker run --rm \
  -v "$PWD:/src:ro" -v "$CACHE/target:/target" -v "$CACHE/registry:/usr/local/cargo/registry" -v "$CACHE/git:/usr/local/cargo/git" \
  -e CARGO_TARGET_DIR=/target -w /src "$IMAGE-build" \
  sh -c 'triple="$(uname -m)-unknown-linux-musl" && cargo build --release --locked --no-default-features --quiet --target "$triple" && cp "/target/$triple/release/prism" /target/prism-musl'
BINARY="$CACHE/target/prism-musl"

# A throwaway key, used only through the generated ssh_config.
[[ -f "$CACHE/id_ed25519" ]] || ssh-keygen -q -t ed25519 -N '' -C prism-e2e -f "$CACHE/id_ed25519"

docker build -q -t "$IMAGE" - >/dev/null <<'DOCKERFILE'
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends openssh-server && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /run/sshd && useradd -m -s /bin/bash me \
 && printf 'PasswordAuthentication no\nKbdInteractiveAuthentication no\n' >> /etc/ssh/sshd_config
CMD ["/usr/sbin/sshd", "-D", "-e"]
DOCKERFILE

docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" -p 127.0.0.1::22 "$IMAGE" >/dev/null
docker exec "$NAME" install -d -o me -g me -m 700 /home/me/.ssh /home/me/.local /home/me/.local/bin
docker cp "$CACHE/id_ed25519.pub" "$NAME:/home/me/.ssh/authorized_keys"
docker cp "$BINARY" "$NAME:/home/me/.local/bin/prism"
docker exec "$NAME" chown -R me:me /home/me/.ssh /home/me/.local
docker exec "$NAME" chmod 600 /home/me/.ssh/authorized_keys
PORT="$(docker port "$NAME" 22/tcp | head -1 | sed 's/.*://')"

# A new container has a new host key: start from an empty known_hosts.
rm -f "$CACHE/known_hosts"
cat >"$CACHE/ssh_config" <<CONFIG
Host $NAME
  HostName 127.0.0.1
  Port $PORT
  User me
  IdentityFile $CACHE/id_ed25519
  IdentitiesOnly yes
  UserKnownHostsFile $CACHE/known_hosts
  StrictHostKeyChecking accept-new
  LogLevel ERROR
CONFIG
# The binary, where `prism hosts install --binary` finds it.
cp "$BINARY" "$CACHE/prism-linux"

for _ in $(seq 1 30); do
  if ssh -F "$CACHE/ssh_config" -o BatchMode=yes "$NAME" true 2>/dev/null; then
    echo "$NAME (ssh -F $CACHE/ssh_config $NAME)"
    exit 0
  fi
  sleep 1
done
echo "sshd in $NAME did not answer" >&2
exit 1
