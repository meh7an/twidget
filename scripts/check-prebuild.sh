#!/usr/bin/env bash
# Checks the libraries the build scripts left in prebuild/: the macOS library holds every
# architecture, each library defines every function include/twidget.h declares and has its line in
# BUILD_INFO, and a C program built with the header against the macOS library resolves every
# fixture to its golden tree on each architecture this Mac runs. Each Android library must be one
# object that defines nothing else globally and has no section groups, and the same program must
# link against it beside another rust_eh_personality while twidget keeps its own. Written for the
# bash 3.2 that macOS ships.
set -euo pipefail

cd "$(dirname "$0")/.."
source scripts/prebuild.sh

# The environment the goldens were resolved in, Env::TEST in src/expr/eval.rs.
NOW_MS=1790415687123
TZ_OFFSET_MINUTES=210
# Where the C programs are built, beside the other build products, and how strictly.
BUILD=target/c
C_FLAGS=(-std=c11 -Wall -Wextra -Werror -pedantic)
MACOS="$OUTPUT/lib/macos/$LIBRARY"

require_ndk
read_header_functions

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
    for function in "${FUNCTIONS[@]}"; do
        if ! grep -qx "$prefix$function" <<<"$symbols"; then
            echo "$library $*: missing $prefix$function" >&2
            exit 1
        fi
    done
}

# Stops unless the Android library at `$1` holds one object whose global definitions are exactly
# the header's functions, as text, and which has no section groups left.
require_self_contained() {
    local library="$1"
    local members
    local globals
    local groups
    members="$("$NDK_AR" t "$library" | awk 'END { print NR }')"
    if ((members != 1)); then
        echo "$library: $members members, not one" >&2
        exit 1
    fi
    globals="$("$NDK_NM" -g --defined-only "$library" | awk 'NF == 3 { print $2, $3 }' | sort)"
    if [[ "$globals" != "$(printf 'T %s\n' "${FUNCTIONS[@]}" | sort)" ]]; then
        echo "$library: its global definitions are not exactly the header's functions:" >&2
        echo "$globals" >&2
        exit 1
    fi
    groups="$("$NDK_READELF" -g "$library")"
    if grep -q 'group section' <<<"$groups"; then
        echo "$library: section groups are left" >&2
        exit 1
    fi
}

# Prints, as a number, the little-endian word of `$3` bytes that the linker wrote at address `$2`
# of the binary `$1`.
read_word() {
    local binary="$1"
    local address="$2"
    local size="$3"
    local start offset length byte
    local word=""
    while read -r start offset length; do
        if ((16#$start <= address && address < 16#$start + 16#$length)); then
            for byte in $(od -A n -t x1 -j "$((16#$offset + address - 16#$start))" -N "$size" \
                "$binary"); do
                word="$byte$word"
            done
            echo "$((16#$word))"
            return
        fi
    done < <("$NDK_READELF" -S -W "$binary" | sed -nE 's/^ *\[ *[0-9]+\] +//p' |
        awk '$2 == "PROGBITS" { print $3, $4, $5 }')
    echo "$binary: no section holds address $address" >&2
    exit 1
}

# Stops unless every unwind entry of the binary at `$1` that names a personality routine names
# twidget's own, the one local rust_eh_personality, rather than the stand-in's global one. ARM's
# unwind tables name the routine itself, and the other ABIs' CIEs name a word holding its address.
require_own_personality() {
    local binary="$1"
    local symbols
    local header
    local own
    local stand_in
    local word_size=4
    local address
    local routine
    local routines=()
    symbols="$("$NDK_READELF" -s -W "$binary")"
    own="$(awk '$4 == "FUNC" && $5 == "LOCAL" && $8 == "rust_eh_personality" { print $2 }' \
        <<<"$symbols")"
    stand_in="$(awk '$4 == "FUNC" && $5 == "GLOBAL" && $8 == "rust_eh_personality" { print $2 }' \
        <<<"$symbols")"
    if [[ ! "$own" =~ ^[0-9a-f]+$ || ! "$stand_in" =~ ^[0-9a-f]+$ ]] ||
        ((16#$own == 16#$stand_in)); then
        echo "$binary: no separate local and global rust_eh_personality" >&2
        exit 1
    fi
    header="$("$NDK_READELF" -h "$binary")"
    if [[ "$header" == *ELF64* ]]; then
        word_size=8
    fi
    while read -r address; do
        routines+=("$((16#${address#0x}))")
    done < <("$NDK_READOBJ" -u "$binary" |
        awk '$1 == "PersonalityRoutineAddress:" { print $2 }' | sort -u)
    while read -r address; do
        routine="$(read_word "$binary" "$((16#$address))" "$word_size")"
        routines+=("$routine")
    done < <("$NDK_OBJDUMP" --dwarf=frames "$binary" |
        awk '$1 == "Personality" && $2 == "Address:" { print $3 }' | sort -u)
    if ((${#routines[@]} == 0)); then
        echo "$binary: no unwind entry names a personality routine" >&2
        exit 1
    fi
    for routine in "${routines[@]}"; do
        if ((routine != 16#$own)); then
            echo "$binary: an unwind entry names the personality routine at" \
                "$(printf %x "$routine"), not twidget's at $own" >&2
            exit 1
        fi
    done
    echo "twidget's unwind entries name its own personality routine at $own, not the stand-in's" \
        "at $stand_in"
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
    if ! grep -qE "^lib/$abi/$LIBRARY .* exports=${#FUNCTIONS[@]}( |$)" "$BUILD_INFO"; then
        echo "$BUILD_INFO lacks exports=${#FUNCTIONS[@]} for lib/$abi/$LIBRARY; run" \
            "scripts/build-android.sh" >&2
        exit 1
    fi
    require_self_contained "$OUTPUT/lib/$abi/$LIBRARY"
done
echo "every library defines the ${#FUNCTIONS[@]} functions of include/twidget.h, and each Android" \
    "library is one object that defines nothing else globally and has no section groups"

# Linking for the oldest macOS with the linker's warnings fatal also proves that no object in the
# library needs a newer one.
echo "==> tests/c/resolve.c against $MACOS"
arch_flags=()
for arch in "${MACOS_ARCHS[@]}"; do
    arch_flags+=(-arch "$arch")
done
mkdir -p "$BUILD"
MACOSX_DEPLOYMENT_TARGET="$MACOS_DEPLOYMENT_TARGET" cc "${arch_flags[@]}" "${C_FLAGS[@]}" \
    -Wl,-fatal_warnings -I include tests/c/resolve.c "$MACOS" -o "$BUILD/resolve"
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

# Each Android library links into the same program for its ABI and API level beside a stand-in
# that defines a global rust_eh_personality, as tlottie does, with the linker's warnings fatal.
# Applying the dynamic relocations at link time leaves each CIE's personality word in the file.
printf 'void rust_eh_personality(void) {}\n' >"$BUILD/personality.c"
for index in "${!ANDROID_ABIS[@]}"; do
    abi="${ANDROID_ABIS[$index]}"
    target="--target=${ANDROID_CLANG_TARGETS[$index]}$API_LEVEL"
    echo "==> tests/c/resolve.c against $OUTPUT/lib/$abi/$LIBRARY and a stand-in personality"
    mkdir -p "$BUILD/$abi"
    "$NDK_CLANG" "$target" "${C_FLAGS[@]}" -c "$BUILD/personality.c" -o "$BUILD/$abi/personality.o"
    "$NDK_CLANG" "$target" "${C_FLAGS[@]}" -Wl,--fatal-warnings -Wl,--apply-dynamic-relocs \
        -I include tests/c/resolve.c "$BUILD/$abi/personality.o" "$OUTPUT/lib/$abi/$LIBRARY" \
        -o "$BUILD/$abi/resolve"
    require_own_personality "$BUILD/$abi/resolve"
done

echo "==> the clock fixture through the C API"
"$BUILD/resolve" fixtures/envelopes/clock.json fixtures/templates "$NOW_MS" "$TZ_OFFSET_MINUTES" \
    light ltr
