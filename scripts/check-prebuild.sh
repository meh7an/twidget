#!/usr/bin/env bash
# Checks the libraries the build scripts left in prebuild/: the macOS library holds every
# architecture, each library defines every function include/twidget.h declares and has its line in
# BUILD_INFO, and a C program built with the header against the macOS library resolves every
# fixture to its golden tree on each architecture this Mac runs. Written for the bash 3.2 that
# macOS ships.
set -euo pipefail

cd "$(dirname "$0")/.."
source scripts/prebuild.sh

# The environment the goldens were resolved in, Env::TEST in src/expr/eval.rs.
NOW_MS=1790415687123
TZ_OFFSET_MINUTES=210
# Where the C program is built, beside the other build products.
BUILD=target/c
MACOS="$OUTPUT/lib/macos/$LIBRARY"

# The functions the header declares, each on a line that starts with its return type.
functions=()
while read -r function; do
    functions+=("$function")
done < <(grep -E '^[A-Za-z].*twidget_[a-z_]+\(' include/twidget.h |
    sed -E 's/.*(twidget_[a-z_]+)\(.*/\1/')
if ((${#functions[@]} == 0)); then
    echo "no functions found in include/twidget.h" >&2
    exit 1
fi

# Stops unless the library at `$1` under $OUTPUT exists and BUILD_INFO has its line, which it
# prints; `$2` is the script that builds it.
require_built() {
    if [[ ! -f "$OUTPUT/$1" ]]; then
        echo "missing $OUTPUT/$1; run $2" >&2
        exit 1
    fi
    if ! grep "^$1 " "$BUILD_INFO"; then
        echo "$BUILD_INFO has no line for $1; run $2" >&2
        exit 1
    fi
}

# Stops unless the library at `$1` defines every header function as an external text symbol,
# spelled with the prefix `$2` its object format adds; any further arguments go to nm.
require_exports() {
    local library="$1"
    local prefix="$2"
    shift 2
    local symbols
    symbols="$(nm -g --defined-only --no-llvm-bc --quiet "$@" "$library" |
        awk 'NF == 3 && $2 == "T" { print $3 }')"
    for function in "${functions[@]}"; do
        if ! grep -qx "$prefix$function" <<<"$symbols"; then
            echo "$library $*: missing $prefix$function" >&2
            exit 1
        fi
    done
}

echo "==> $MACOS"
require_built "lib/macos/$LIBRARY" scripts/build-macos.sh
lipo -info "$MACOS"
for arch in "${MACOS_ARCHS[@]}"; do
    if ! lipo "$MACOS" -verify_arch "$arch"; then
        echo "$MACOS lacks $arch" >&2
        exit 1
    fi
    require_exports "$MACOS" _ --arch="$arch"
done

for abi in "${ANDROID_ABIS[@]}"; do
    echo "==> $OUTPUT/lib/$abi/$LIBRARY"
    require_built "lib/$abi/$LIBRARY" scripts/build-android.sh
    require_exports "$OUTPUT/lib/$abi/$LIBRARY" ""
done
echo "every library defines the ${#functions[@]} functions of include/twidget.h"

# Linking for the oldest macOS with the linker's warnings fatal also proves that no object in the
# library needs a newer one.
echo "==> tests/c/resolve.c against $MACOS"
arch_flags=()
for arch in "${MACOS_ARCHS[@]}"; do
    arch_flags+=(-arch "$arch")
done
mkdir -p "$BUILD"
MACOSX_DEPLOYMENT_TARGET="$MACOS_DEPLOYMENT_TARGET" cc "${arch_flags[@]}" -std=c11 -Wall -Wextra \
    -Werror -pedantic -Wl,-fatal_warnings -I include tests/c/resolve.c "$MACOS" -o "$BUILD/resolve"
for arch in "${MACOS_ARCHS[@]}"; do
    if ! arch "-$arch" /usr/bin/true 2>/dev/null; then
        echo "$arch: skipped, since this Mac cannot run it"
        continue
    fi
    for golden in fixtures/expected/resolved/*.json; do
        IFS=. read -r fixture theme direction _ <<<"$(basename "$golden")"
        arch "-$arch" "$BUILD/resolve" "fixtures/envelopes/$fixture.json" fixtures/templates \
            "$NOW_MS" "$TZ_OFFSET_MINUTES" "$theme" "$direction" >"$BUILD/resolved.json"
        if ! cmp "$BUILD/resolved.json" "$golden"; then
            echo "$arch: the C API's tree differs from $golden" >&2
            exit 1
        fi
    done
    echo "$arch: every tree matches its golden in fixtures/expected/resolved"
done

echo "==> the clock fixture through the C API"
"$BUILD/resolve" fixtures/envelopes/clock.json fixtures/templates "$NOW_MS" "$TZ_OFFSET_MINUTES" \
    light ltr
