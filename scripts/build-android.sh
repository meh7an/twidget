#!/usr/bin/env bash
# Builds a static library for each Android ABI at prebuild/lib/<ABI>, through cargo-ndk with the
# pinned NDK from the Android SDK at ANDROID_HOME, or at its default place when that is unset.
# Written for the bash 3.2 that macOS ships.
set -euo pipefail

cd "$(dirname "$0")/.."
source scripts/prebuild.sh

# cargo-ndk picks the newest NDK in the SDK unless this names one.
export ANDROID_NDK_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}/ndk/$NDK_VERSION"
if [[ ! -d "$ANDROID_NDK_HOME" ]]; then
    echo "missing NDK $NDK_VERSION at $ANDROID_NDK_HOME" >&2
    exit 1
fi
if ! command -v cargo-ndk >/dev/null; then
    echo "missing cargo-ndk; run: cargo install cargo-ndk --locked" >&2
    exit 1
fi
require_targets "${ANDROID_TARGETS[@]}"

for index in "${!ANDROID_ABIS[@]}"; do
    abi="${ANDROID_ABIS[$index]}"
    echo "==> $abi"
    cargo ndk --target "$abi" --platform "$API_LEVEL" rustc -p "$CRATE" --lib --release --locked \
        --features "$FEATURES" --crate-type staticlib
    mkdir -p "$OUTPUT/lib/$abi"
    cp "target/${ANDROID_TARGETS[$index]}/release/$LIBRARY" "$OUTPUT/lib/$abi/$LIBRARY"
    record_build "lib/$abi/$LIBRARY" "ndk=$NDK_VERSION" "api=$API_LEVEL"
done
