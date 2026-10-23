#!/usr/bin/env bash
# Builds a static library for each Android ABI at prebuild/lib/<ABI>, through cargo-ndk with the
# pinned NDK, then turns it into one object whose only global symbols are the header's functions.
# Written for the bash 3.2 that macOS ships.
set -euo pipefail

cd "$(dirname "$0")/.."
source scripts/prebuild.sh

require_ndk
# cargo-ndk picks the newest NDK in the SDK unless this names one.
export ANDROID_NDK_HOME="$NDK"
if ! command -v cargo-ndk >/dev/null; then
    echo "missing cargo-ndk; run: cargo install cargo-ndk --locked" >&2
    exit 1
fi
require_targets "${ANDROID_TARGETS[@]}"
read_header_functions
keep=()
for function in "${FUNCTIONS[@]}"; do
    keep+=("--keep-global-symbol=$function")
done

for index in "${!ANDROID_ABIS[@]}"; do
    abi="${ANDROID_ABIS[$index]}"
    release="target/${ANDROID_TARGETS[$index]}/release"
    library="$OUTPUT/lib/$abi/$LIBRARY"
    echo "==> $abi"
    cargo ndk --target "$abi" --platform "$API_LEVEL" rustc -p "$CRATE" --lib --release --locked \
        --features "$FEATURES" --crate-type staticlib
    # The app links this beside other Rust libraries, so every member joins one object in which
    # each definition but the header's functions turns local, Rust std's personality routine and
    # globals among them. Removing a section group keeps its sections but takes them out of it, so
    # the app's linker cannot trade twidget's copy of a group, such as
    # DW.ref.rust_eh_personality's, for another library's. The compiler_builtins members' embedded
    # bitcode, joined into one section that no longer parses, goes too; final links drop it anyway.
    "$NDK_LLD" -r --whole-archive "$release/$LIBRARY" -o "$release/$CRATE.o"
    "$NDK_OBJCOPY" --remove-section=.group --remove-section=.llvmbc --remove-section=.llvmcmd \
        "${keep[@]}" "$release/$CRATE.o"
    mkdir -p "$OUTPUT/lib/$abi"
    rm -f "$library"
    "$NDK_AR" rcsD "$library" "$release/$CRATE.o"
    exports="$("$NDK_NM" -g --defined-only "$library" |
        awk 'NF == 3 { count++ } END { print count + 0 }')"
    record_build "lib/$abi/$LIBRARY" "ndk=$NDK_VERSION" "api=$API_LEVEL" "exports=$exports"
done
