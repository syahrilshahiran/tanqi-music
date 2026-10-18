#!/usr/bin/env bash
# `dx build` rewrites the Android launcher icons with its default ones every time. Run this after it to put
# ours (from icons/android/) back and assemble the APK again, in the same place dx puts it.
# Needs the same ANDROID_HOME and JAVA_HOME as the build.
set -euo pipefail
cd "$(dirname "$0")/.."
APP=target/dx/tanqi-music/debug/android/app
RES=$APP/app/src/main/res
for d in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
  rm -f "$RES"/mipmap-$d/ic_launcher*.webp
  cp icons/android/mipmap-$d/*.png "$RES"/mipmap-$d/
done
rm -f "$RES"/drawable/ic_launcher_background.xml "$RES"/drawable-v24/ic_launcher_foreground.xml
cp icons/android/mipmap-anydpi-v26/ic_launcher.xml "$RES"/mipmap-anydpi-v26/
cp icons/android/values/ic_launcher_background.xml "$RES"/values/
(cd "$APP" && ./gradlew assembleDebug -q)
echo "$APP/app/build/outputs/apk/debug/app-debug.apk"
