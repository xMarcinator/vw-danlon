#!/usr/bin/env bash
# Build the arm64 release APK and sign it with the local release keystore.
# Output: dist/vw-danlon-<version>-arm64.apk
#
# Keystore and password live outside the repo (override with env):
#   VW_DANLON_KEYSTORE       default ~/.config/vw-danlon/release.jks
#   VW_DANLON_KEYSTORE_PASS  default ~/.config/vw-danlon/release.jks.pass
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
keystore=${VW_DANLON_KEYSTORE:-$HOME/.config/vw-danlon/release.jks}
passfile=${VW_DANLON_KEYSTORE_PASS:-$HOME/.config/vw-danlon/release.jks.pass}

export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)}
export JAVA_HOME=${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk}
build_tools=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)

# The NDK compiler for C dependencies (ring) on aarch64 Android, API 24.
ndk_bin=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
export CC_aarch64_linux_android=$ndk_bin/aarch64-linux-android24-clang
export AR_aarch64_linux_android=$ndk_bin/llvm-ar
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$CC_aarch64_linux_android

for f in "$keystore" "$passfile"; do
  [ -f "$f" ] || { echo "missing $f (see README: Android phone app)" >&2; exit 1; }
done

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/app/Cargo.toml" | head -1)

cd "$repo/app"
dx bundle --platform android --release --target aarch64-linux-android --package-types apk

# dx 0.7 packages its Gradle *debug* variant (debuggable, unstripped .so).
# Rebuild the generated project's release variant instead: strip the native
# library, turn off R8 (the generated project has no keep rules for the
# classes Rust calls over JNI), skip lint (needs network), sign below.
gradle_dir=$repo/target/dx/vw-danlon-app/release/android/app
"$ndk_bin/llvm-strip" --strip-all "$gradle_dir/app/src/main/jniLibs/arm64-v8a/libmain.so"
sed -i 's/isMinifyEnabled = true/isMinifyEnabled = false/' "$gradle_dir/app/build.gradle.kts"
(cd "$gradle_dir" && ./gradlew assembleRelease -q \
  -x lintVitalAnalyzeRelease -x lintVitalReportRelease -x lintVitalRelease)
apk=$gradle_dir/app/build/outputs/apk/release/app-release-unsigned.apk
mkdir -p "$repo/dist"
out=$repo/dist/vw-danlon-$version-arm64.apk
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

"$build_tools/zipalign" -f -p 4 "$apk" "$tmp/aligned.apk"
"$build_tools/apksigner" sign \
  --ks "$keystore" --ks-key-alias vw-danlon \
  --ks-pass "file:$passfile" \
  --out "$out" "$tmp/aligned.apk"
"$build_tools/apksigner" verify --print-certs "$out" | grep -E 'Signer #1 certificate (DN|SHA-256)'
if "$build_tools/aapt2" dump badging "$out" | grep -q application-debuggable; then
  echo "$out is debuggable" >&2; exit 1
fi
ls -lh "$out"
