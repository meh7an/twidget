#!/usr/bin/env bash
# Every check a milestone must pass: formatting, then clippy with warnings denied and the tests,
# once for each combination of the crate's features. Written for the bash 3.2 that macOS ships.
set -euo pipefail

FEATURES=(std c-api wasm cli)

cd "$(dirname "$0")/.."

cargo fmt --check

count=${#FEATURES[@]}
for ((mask = 0; mask < 1 << count; mask++)); do
    selected=""
    for ((bit = 0; bit < count; bit++)); do
        if ((mask & 1 << bit)); then
            selected="${selected:+$selected,}${FEATURES[bit]}"
        fi
    done
    echo "==> features: ${selected:-none}"
    cargo clippy --all-targets --no-default-features --features "$selected" -- -D warnings
    cargo test --no-default-features --features "$selected"
done
