#!/bin/bash
# macOS CPU stack sampling, with the existing paced simulation probe as the workload.
set -euo pipefail

if [ "$#" -ne 2 ]; then
    printf 'Usage: bash tools/profile_simulation.sh SCENE OUTPUT_DIRECTORY\n' >&2
    exit 2
fi
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
scene="$1"
output="$2"
probe="target/release/io-worker-probe"
if [ ! -x "$probe" ] || [ ! -f "$scene" ]; then
    printf 'Build the release io-worker-probe and provide an existing scene.\n' >&2
    exit 2
fi
if [ -e "$output" ]; then
    printf 'Output already exists; choose a new directory.\n' >&2
    exit 2
fi
mkdir -p "$output"
shasum -a 256 "$probe" "$scene" > "$output/inputs.sha256"
uname -a > "$output/platform.txt"
date -u >> "$output/platform.txt"
"$probe" "$scene" 3600 > "$output/probe.json" 2> "$output/probe.log" &
pid=$!
cleanup() {
    if kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
    fi
}
trap cleanup EXIT
# Sample steady movement rather than initial loading/admission alone.
sleep 20
/usr/bin/sample "$pid" 20 1 -file "$output/stacks.txt" > "$output/sample.log" 2>&1
wait "$pid"
trap - EXIT
printf 'Profile and paced tick report: %s\n' "$output"
