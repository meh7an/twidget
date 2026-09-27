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
# index, with the NDK and the API level its TMessagesProj/build.gradle sets.
ANDROID_ABIS=(armeabi-v7a arm64-v8a x86 x86_64)
ANDROID_TARGETS=(armv7-linux-androideabi aarch64-linux-android i686-linux-android
    x86_64-linux-android)
NDK_VERSION=27.2.12479018
API_LEVEL=21

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
