#!/usr/bin/env bash
# Builds the universal macOS static library at prebuild/lib/macos: each macOS target in release,
# joined by lipo. Written for the bash 3.2 that macOS ships.
set -euo pipefail

cd "$(dirname "$0")/.."
source scripts/prebuild.sh

export MACOSX_DEPLOYMENT_TARGET="$MACOS_DEPLOYMENT_TARGET"

require_targets "${MACOS_TARGETS[@]}"
slices=()
for target in "${MACOS_TARGETS[@]}"; do
    echo "==> $target"
    cargo rustc -p "$CRATE" --lib --release --locked --features "$FEATURES" --target "$target" \
        --crate-type staticlib
    slices+=("target/$target/release/$LIBRARY")
done
mkdir -p "$OUTPUT/lib/macos"
lipo -create -output "$OUTPUT/lib/macos/$LIBRARY" "${slices[@]}"
lipo -info "$OUTPUT/lib/macos/$LIBRARY"
record_build "lib/macos/$LIBRARY" "macos=$MACOS_DEPLOYMENT_TARGET"
