#!/usr/bin/env bash
# Builds the Android APK with the app's own launcher icon (icons/android/).
#
# `dx build` writes its default launcher icon every time, and refuses to build while a previous run's icon
# files sit next to it, so this clears ours, builds, puts ours back, and assembles the APK again.
# Needs ANDROID_HOME, ANDROID_NDK_HOME and JAVA_HOME, as in the README. Prints the APK's path.
#
#   icons/android-build.sh [rust target]      (default: aarch64-linux-android, for a phone)
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET="${1:-aarch64-linux-android}"
APP=target/dx/tanqi-music/debug/android/app
RES=$APP/app/src/main/res

# 1. clear what an earlier run put in
for d in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
  rm -f "$RES"/mipmap-$d/ic_launcher.png "$RES"/mipmap-$d/ic_launcher_round.png "$RES"/mipmap-$d/ic_launcher_foreground.png
done
rm -f "$RES"/values/ic_launcher_background.xml

# 2. dx's own build (default icon)
dx build --platform android --no-default-features --features mobile --target "$TARGET"

# 3. swap in ours and assemble again
for d in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
  rm -f "$RES"/mipmap-$d/ic_launcher*.webp
  cp icons/android/mipmap-$d/*.png "$RES"/mipmap-$d/
done
rm -f "$RES"/drawable/ic_launcher_background.xml "$RES"/drawable-v24/ic_launcher_foreground.xml
cp icons/android/mipmap-anydpi-v26/ic_launcher.xml "$RES"/mipmap-anydpi-v26/
cp icons/android/values/ic_launcher_background.xml "$RES"/values/
(cd "$APP" && ./gradlew assembleDebug -q)
echo "$APP/app/build/outputs/apk/debug/app-debug.apk"
