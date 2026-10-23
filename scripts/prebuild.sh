# The settings and helpers every release script shares, sourced at its top: what is built, for
# which platforms, and where it lands. Merging twidget with tlottie into one umbrella library, or
# moving to a new NDK, is an edit here. Written for the bash 3.2 that macOS ships.

# The Cargo package, the static library it builds, and the features the apps link.
CRATE=twidget
LIBRARY=libtwidget.a
FEATURES=c-api

# Where the libraries and their build record go, from the repository root; git ignores it. Each
# library lands at lib/<platform>/$LIBRARY, the platform being macos or an Android ABI.
OUTPUT=prebuild
BUILD_INFO="$OUTPUT/BUILD_INFO"

# macOS: one universal library from these targets, whose architectures lipo names at the same
# index, for the oldest macOS that Telegram Desktop's prepare.py builds tlottie for.
MACOS_TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
MACOS_ARCHS=(arm64 x86_64)
MACOS_DEPLOYMENT_TARGET=10.13

# Android: a library for each ABI Telegram Android ships, built by the Rust target at the same
# index and linked in checks by the NDK's clang for the triple there, with the NDK and the API
# level its TMessagesProj/build.gradle sets.
ANDROID_ABIS=(armeabi-v7a arm64-v8a x86 x86_64)
ANDROID_TARGETS=(armv7-linux-androideabi aarch64-linux-android i686-linux-android
    x86_64-linux-android)
ANDROID_CLANG_TARGETS=(armv7a-linux-androideabi aarch64-linux-android i686-linux-android
    x86_64-linux-android)
NDK_VERSION=27.2.12479018
API_LEVEL=21

# The NDK in the Android SDK at ANDROID_HOME, or at its default place when that is unset, and the
# LLVM tools from it that build and check the Android libraries; its macOS tools sit under
# darwin-x86_64 on every Mac.
NDK="${ANDROID_HOME:-$HOME/Library/Android/sdk}/ndk/$NDK_VERSION"
NDK_BIN="$NDK/toolchains/llvm/prebuilt/darwin-x86_64/bin"
NDK_AR="$NDK_BIN/llvm-ar"
NDK_CLANG="$NDK_BIN/clang"
NDK_LLD="$NDK_BIN/ld.lld"
NDK_NM="$NDK_BIN/llvm-nm"
NDK_OBJCOPY="$NDK_BIN/llvm-objcopy"
NDK_OBJDUMP="$NDK_BIN/llvm-objdump"
NDK_READELF="$NDK_BIN/llvm-readelf"
NDK_READOBJ="$NDK_BIN/llvm-readobj"

# Records how the library at `$1`, a path under $OUTPUT, was built, with any further arguments as
# extra fields, replacing the line an earlier build left for it.
record_build() {
    local artifact="$1"
    shift
    local commit
    commit="$(git rev-parse HEAD)"
    if ! git diff --quiet HEAD; then
        commit="$commit-dirty"
    fi
    mkdir -p "$OUTPUT"
    touch "$BUILD_INFO"
    {
        grep -v "^$artifact " "$BUILD_INFO" || true
        echo "$artifact commit=$commit rustc=$(rustc --version | cut -d' ' -f2) $*"
    } | sort >"$BUILD_INFO.new"
    mv "$BUILD_INFO.new" "$BUILD_INFO"
}

# Sets FUNCTIONS to the functions include/twidget.h declares, each on a line that starts with its
# return type, and stops if it finds none.
read_header_functions() {
    local function
    FUNCTIONS=()
    while read -r function; do
        FUNCTIONS+=("$function")
    done < <(grep -E '^[A-Za-z].*twidget_[a-z_]+\(' include/twidget.h |
        sed -E 's/.*(twidget_[a-z_]+)\(.*/\1/')
    if ((${#FUNCTIONS[@]} == 0)); then
        echo "no functions found in include/twidget.h" >&2
        exit 1
    fi
}

# Stops unless the pinned NDK is installed.
require_ndk() {
    if [[ ! -d "$NDK" ]]; then
        echo "missing NDK $NDK_VERSION at $NDK" >&2
        exit 1
    fi
}

# Stops unless every Rust target named is installed for the pinned toolchain.
require_targets() {
    local installed
    local missing=""
    installed="$(rustup target list --installed)"
    for target in "$@"; do
        if ! grep -qx "$target" <<<"$installed"; then
            missing="$missing $target"
        fi
    done
    if [[ -n "$missing" ]]; then
        echo "missing Rust targets; run: rustup target add$missing" >&2
        exit 1
    fi
}
