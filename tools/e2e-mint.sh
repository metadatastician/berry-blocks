#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# e2e-mint.sh — mint a plugin through the running wizard, end to end, in a
# scratch copy of this checkout, then hold the generated crate to the repo's
# own gates (fmt, clippy -D warnings, its conformance test). The real checkout
# is never written. Requires a built `berry-blocks` binary and fetched pins.
set -euo pipefail
cd "$(dirname "$0")/.."

ROOT=$PWD
SCRATCH=$(mktemp -d)
PORT=${E2E_PORT:-23899}
ADDR="127.0.0.1:${PORT}"
NAME=${E2E_NAME:-zebra-notes}
SERVER_PID=""

# cleanup — stops the wizard and removes the scratch copy on any exit.
cleanup() {
    [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
    rm -rf "$SCRATCH"
}
trap cleanup EXIT

# post FORM PATH — POSTs a form to the wizard as a same-origin browser would.
post() {
    curl -sS -X POST -H "Origin: http://${ADDR}" --data "$1" "http://${ADDR}$2" "${@:3}"
}

git ls-files -z | xargs -0 -I{} cp --parents {} "$SCRATCH"/
cp -r vendor "$SCRATCH"/

"$ROOT/target/debug/berry-blocks" wizard --root "$SCRATCH" --addr "$ADDR" &
SERVER_PID=$!
for _ in $(seq 1 50); do curl -s -o /dev/null "http://${ADDR}/" && break; sleep 0.1; done

FORM="name=${NAME}&display=E2E+${NAME}&claims=zkey&run_key=group&licence=MPL-2.0"
DIGEST=$(post "$FORM" /mint/preview | grep -oE 'name="digest" value="[0-9a-f]{64}"' | grep -oE '[0-9a-f]{64}')
[[ "$DIGEST" =~ ^[0-9a-f]{64}$ ]] || { echo "e2e-mint: preview gave no digest" >&2; exit 1; }

STALE=$(post "${FORM/E2E/Edited}&digest=${DIGEST}" /mint -o /dev/null -w '%{http_code}')
[ "$STALE" = 409 ] || { echo "e2e-mint: an edited form was not refused (got ${STALE})" >&2; exit 1; }
[ ! -e "$SCRATCH/plugins/${NAME}" ] || { echo "e2e-mint: a refused mint wrote files" >&2; exit 1; }

CODE=$(post "${FORM}&digest=${DIGEST}" /mint -o /dev/null -w '%{http_code}')
[ "$CODE" = 303 ] || { echo "e2e-mint: mint returned ${CODE}" >&2; exit 1; }

cd "$SCRATCH"
CRATE="berry-blocks-${NAME}"
cargo fmt --check -p "$CRATE"
cargo fmt --check -p berry-blocks-registry
CARGO_TARGET_DIR="$ROOT/target" cargo clippy -q -p "$CRATE" -p berry-blocks-registry --all-targets -- -D warnings
CARGO_TARGET_DIR="$ROOT/target" cargo test -q -p "$CRATE" -p berry-blocks-registry
grep -q "name: \"${NAME}\"" crates/berry-blocks-registry/src/lib.rs || { echo "e2e-mint: the plugin was not registered" >&2; exit 1; }
echo "e2e-mint: minted ${NAME} through the wizard; the generated crate and the registry pass fmt, clippy and their tests"
