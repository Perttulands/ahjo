#!/usr/bin/env bash
# Checks the holder on Windows from WSL in a private workspace under %TEMP%.
# Arguments go to `cargo test` (for example `-- --nocapture`).
# AHJO_HOLDER_SCRATCH supplies the scratch directory's prefix.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cmd=/mnt/c/Windows/System32/cmd.exe
temp=$(wslpath "$(cd /mnt/c && "$cmd" /d /c 'echo %TEMP%' | tr -d '\r')")
[[ "$temp" == /mnt/* ]] || { echo "no Windows %TEMP% found through $cmd" >&2; exit 1; }
prefix=${AHJO_HOLDER_SCRATCH:-ahjo-holder}
case "$prefix" in
    ''|.|..|*/*|*\\*) echo "AHJO_HOLDER_SCRATCH must be a single directory name" >&2; exit 1 ;;
esac
dir=$(mktemp -d "$temp/$prefix.XXXXXXXX")
trap 'rm -rf "$dir"' EXIT
# Cargo needs every workspace member's manifest and the local crate patches.
# Copy tracked files only, including local edits, and omit editor symlinks.
git -C "$root" ls-files -z -- . ':!.agents' ':!.claude' |
    rsync -r --from0 --files-from=- "$root/" "$dir/"
mkdir -p "$dir/temp"
cargo=$(wslpath "$(cd /mnt/c && "$cmd" /d /c 'where cargo.exe' | tr -d '\r' | sed -n '1p')")
cd "$dir"
# Windows tests and rustc must use this invocation's own scratch directory.
export TEMP="$dir/temp" TMP="$dir/temp"
export WSLENV="${WSLENV:+$WSLENV:}TEMP/p:TMP/p"
"$cargo" fmt -p ahjo-holder --check
"$cargo" clippy -p ahjo-holder --all-targets -- -D warnings
"$cargo" test -p ahjo-holder "$@"
