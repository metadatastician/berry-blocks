#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# fetch-pins.sh — check out each project named in pins.kyaml at its pinned
# commit into vendor/<name>/ (gitignored). Idempotent: a vendor copy already at
# the pinned commit is left alone; one at any other commit is re-fetched.
# Requires git and yq (mikefarah v4).
set -euo pipefail
cd "$(dirname "$0")/.."

# fetch_pin NAME — makes vendor/NAME a shallow checkout of its pinned commit.
fetch_pin() {
    local name=$1 repo commit dir
    repo=$(yq -r ".${name}.repo" pins.kyaml)
    commit=$(yq -r ".${name}.commit" pins.kyaml)
    [[ "$commit" =~ ^[0-9a-f]{40}$ ]] || { echo "fetch-pins: ${name}.commit is not a full SHA: ${commit}" >&2; exit 2; }
    dir="vendor/${name}"
    if [ -d "${dir}/.git" ] && [ "$(git -C "$dir" rev-parse HEAD)" = "$commit" ]; then
        echo "${name}: already at ${commit:0:7}"
        return
    fi
    rm -rf "$dir"
    git init -q "$dir"
    git -C "$dir" fetch -q --depth 1 "$repo" "$commit"
    git -C "$dir" -c advice.detachedHead=false checkout -q FETCH_HEAD
    echo "${name}: fetched ${commit:0:7}"
}

for name in berrywiki progblocks; do
    fetch_pin "$name"
done
