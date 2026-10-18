#!/usr/bin/env bash
# Builds the Android APK with the app's own launcher icon (icons/android/).
#
#   icons/android-build.sh             a debug APK (large, slow, debug-signed): quick to try on a phone
#   icons/android-build.sh --release   an optimised, signed APK (about a tenth of the size): to share
#
# Needs ANDROID_HOME, ANDROID_NDK_HOME and JAVA_HOME, as in the README. Prints the APK's path.
#
# `dx build` writes its default launcher icon every time, and refuses to build while a previous run's icon files
# sit next to it, so this clears ours, builds, puts ours back, and assembles the APK again.
#
# A release APK is signed with the key in ~/.config/tanqi/ (tanqi-release.jks, with its passwords in
# keystore.properties). Keep that key: Android only lets an app update itself when it is signed with the same
# one. It is created on first use if it isn't there, and never lives in the repository.
set -euo pipefail
cd "$(dirname "$0")/.."

MODE=debug
if [ "${1:-}" = "--release" ]; then MODE=release; shift; fi
TARGET="${1:-aarch64-linux-android}"
APP=target/dx/tanqi-music/$MODE/android/app
RES=$APP/app/src/main/res
DXFLAGS=(); [ "$MODE" = release ] && DXFLAGS=(--release)

# 1. clear what an earlier run put in
for d in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
  rm -f "$RES"/mipmap-$d/ic_launcher.png "$RES"/mipmap-$d/ic_launcher_round.png "$RES"/mipmap-$d/ic_launcher_foreground.png
done
rm -f "$RES"/values/ic_launcher_background.xml

# 2. dx's own build (default icon)
dx build "${DXFLAGS[@]}" --platform android --no-default-features --features mobile --target "$TARGET"

# 3. swap in ours
for d in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
  rm -f "$RES"/mipmap-$d/ic_launcher*.webp
  cp icons/android/mipmap-$d/*.png "$RES"/mipmap-$d/
done
rm -f "$RES"/drawable/ic_launcher_background.xml "$RES"/drawable-v24/ic_launcher_foreground.xml
cp icons/android/mipmap-anydpi-v26/ic_launcher.xml "$RES"/mipmap-anydpi-v26/
cp icons/android/values/ic_launcher_background.xml "$RES"/values/

if [ "$MODE" = debug ]; then
  (cd "$APP" && ./gradlew assembleDebug -q)
  echo "$APP/app/build/outputs/apk/debug/app-debug.apk"
  exit 0
fi

# 4. release: assemble unsigned, align, sign
(cd "$APP" && ./gradlew assembleRelease -q)
UNSIGNED=$APP/app/build/outputs/apk/release/app-release-unsigned.apk
KEYDIR="$HOME/.config/tanqi"
KS="$KEYDIR/tanqi-release.jks"
PROPS="$KEYDIR/keystore.properties"
if [ ! -f "$KS" ]; then
  mkdir -p "$KEYDIR"; chmod 700 "$KEYDIR"
  PW=$(openssl rand -base64 24 | tr -d '/+=' | cut -c1-24)
  printf 'storePassword=%s\nkeyPassword=%s\nkeyAlias=tanqi\n' "$PW" "$PW" > "$PROPS"; chmod 600 "$PROPS"
  "$JAVA_HOME/bin/keytool" -genkeypair -keystore "$KS" -storetype PKCS12 -alias tanqi -keyalg RSA -keysize 4096 \
    -validity 10000 -dname "CN=Tanqi, O=Tanqi" -storepass "$PW" -keypass "$PW" >/dev/null 2>&1
  chmod 600 "$KS"
fi
PASS=$(grep '^storePassword=' "$PROPS" | cut -d= -f2)
BT=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)
OUT=$APP/app/build/outputs/apk/release/tanqi-music-release.apk
"$BT/zipalign" -p -f 4 "$UNSIGNED" "$OUT.aligned"
"$BT/apksigner" sign --ks "$KS" --ks-pass "pass:$PASS" --key-pass "pass:$PASS" --out "$OUT" "$OUT.aligned"
rm -f "$OUT.aligned" "$OUT.idsig"
"$BT/apksigner" verify "$OUT"
echo "$OUT"
