#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ZED_DIR="$REPO_ROOT/vendor/zed"
PATCH_DIR="$REPO_ROOT/patches"

if [ ! -d "$ZED_DIR/.git" ] && [ ! -f "$ZED_DIR/.git" ]; then
    echo "vendor/zed not initialized, running git submodule update --init --recursive ..."
    git -C "$REPO_ROOT" submodule update --init --recursive
fi

if [ ! -d "$PATCH_DIR" ]; then
    echo "no patches directory at $PATCH_DIR, nothing to apply."
    exit 0
fi

shopt -s nullglob
patches=("$PATCH_DIR"/*.patch)
shopt -u nullglob

if [ "${#patches[@]}" -eq 0 ]; then
    echo "no .patch files in $PATCH_DIR, nothing to apply."
    exit 0
fi

cd "$ZED_DIR"

for patch in "${patches[@]}"; do
    name="$(basename "$patch")"
    if git apply --check "$patch" >/dev/null 2>&1; then
        echo "applying $name ..."
        git apply "$patch"
    elif git apply --reverse --check "$patch" >/dev/null 2>&1; then
        echo "$name already applied, skipping."
    else
        echo "ERROR: cannot apply $name cleanly. Resolve conflicts manually." >&2
        exit 1
    fi
done

echo "all patches processed."
