#!/usr/bin/env bash
# Installs the test APK on the emulator, opens it, and prints two small screenshots (base64 JPEG
# between markers) and the app's log lines, so the run's log shows how it started.
set -u
# Every adb call has a time limit, so a stuck emulator ends the check instead of the whole job.
a() { timeout 60 adb "$@"; }
command -v convert >/dev/null || sudo apt-get install -y -q imagemagick >/dev/null
apk=$(find src-tauri/gen/android/app/build/outputs/apk -name '*.apk' | head -1)
echo "APK: $apk"
a install -r "$apk"
# Let Google Play services finish starting first: when it restarts during our start-up, Android
# also closes apps that use its fonts.
sleep 60
a logcat -c
# Shut any system dialog left from the emulator's start ("isn't responding").
a shell am broadcast -a android.intent.action.CLOSE_SYSTEM_DIALOGS >/dev/null
a shell am start -n com.recomplauncher.app/.MainActivity
shot() {
  a exec-out screencap -p > "shot-$1.png" || return
  convert "shot-$1.png" -resize 960x -quality 60 "shot-$1.jpg"
  echo "=====SHOT $1 BEGIN====="
  base64 -w0 "shot-$1.jpg"
  echo
  echo "=====SHOT $1 END====="
}
sleep 20
shot 1
sleep 25
shot 2
echo "=====LOG BEGIN====="
a logcat -d | grep -iE 'recomplauncher|RustStdoutStderr|chromium|Tauri|AndroidRuntime|FATAL' | tail -300
echo "=====LOG END====="
