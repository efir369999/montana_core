#!/bin/bash
# The earlier node implementation. A cohort requires an operator manifest;
# an isolated chain requires an explicit mode. Startup never reports to a third party.
set -euo pipefail
set -o noclobber
umask 077

DATA_DIR="${MONTANA_DATA_DIR:-/var/lib/montana}"
MANIFEST="${MONTANA_GENESIS_MANIFEST:-/etc/montana/genesis-manifest.json}"
LISTEN="${MONTANA_LISTEN:-/ip4/0.0.0.0/tcp/8444}"
MODE="${MONTANA_MODE:-cohort}"
recovery_words="${MONTANA_MNEMONIC:-}"
unset MONTANA_MNEMONIC

# Check the requested network before creating an identity or changing stored state.
case "$MODE" in
  cohort)
    if [ ! -f "$MANIFEST" ] || [ ! -r "$MANIFEST" ]; then
      echo '[entrypoint] FATAL: cohort mode requires a readable operator manifest' >&2
      exit 1
    fi
    if [ -n "${MONTANA_MANIFEST_SHA256:-}" ]; then
      actual="$(sha256sum "$MANIFEST")"
      actual="${actual%% *}"
      if [ "$actual" != "$MONTANA_MANIFEST_SHA256" ]; then
        echo '[entrypoint] FATAL: manifest SHA-256 differs from the operator pin' >&2
        exit 1
      fi
    fi
    set -- start --data-dir "$DATA_DIR" --listen "$LISTEN" --genesis-manifest "$MANIFEST"
    ;;
  isolated)
    if [ -e "$MANIFEST" ] || [ -n "${MONTANA_MANIFEST_SHA256:-}" ]; then
      echo '[entrypoint] FATAL: isolated mode conflicts with a cohort manifest or pin' >&2
      exit 1
    fi
    set -- start --data-dir "$DATA_DIR"
    ;;
  *)
    echo '[entrypoint] FATAL: MONTANA_MODE must be cohort or isolated' >&2
    exit 1
    ;;
esac

mkdir -p "$DATA_DIR"
chmod 0700 "$DATA_DIR"
chown montana:montana "$DATA_DIR"

# Recovery words enter through stdin and are written only to a private file.
# The initial umask protects the file from its creation, before init writes a byte.
if [ ! -f "$DATA_DIR/identity.bin" ]; then
  if [ -n "${MONTANA_MNEMONIC_FILE:-}" ]; then
    if [ -n "$recovery_words" ]; then
      echo '[entrypoint] FATAL: configure one mnemonic source' >&2
      exit 1
    fi
    setsid --wait runuser -u montana -- /usr/local/bin/montana-node init --data-dir "$DATA_DIR" --mnemonic-stdin \
      < "$MONTANA_MNEMONIC_FILE" > "$DATA_DIR/mnemonic.txt"
  elif [ -n "$recovery_words" ]; then
    # Compatibility for existing operators. A mounted secret file also keeps
    # the recovery words out of the container's environment configuration.
    printf '%s\n' "$recovery_words" | setsid --wait runuser -u montana -- /usr/local/bin/montana-node init \
      --data-dir "$DATA_DIR" --mnemonic-stdin > "$DATA_DIR/mnemonic.txt"
  else
    setsid --wait runuser -u montana -- /usr/local/bin/montana-node init --data-dir "$DATA_DIR" > "$DATA_DIR/mnemonic.txt"
  fi
  chmod 0400 "$DATA_DIR/mnemonic.txt"
  chown montana:montana "$DATA_DIR/mnemonic.txt"
fi
unset recovery_words MONTANA_MNEMONIC_FILE

if [ -n "${MONTANA_D_TEST_OVERRIDE:-}" ]; then
  set -- "$@" --d-test-override "$MONTANA_D_TEST_OVERRIDE"
  echo '[entrypoint] test difficulty override enabled' >&2
fi

echo "[entrypoint] mode=$MODE; status is local to this node"
exec runuser -u montana -- /usr/local/bin/montana-node "$@"
