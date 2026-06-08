#!/usr/bin/env bash
set -euo pipefail

ROOT="/home/johan/Hem/Programmering/RustroverProjects/Html-Prototype"
APP_DIR="$ROOT/MobileApp"
AVD_NAME="${AVD_NAME:-Pixel_8a_x86_64}"
EMULATOR_SERIAL="${EMULATOR_SERIAL:-emulator-5554}"
JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk}"

if [[ -z "${ANDROID_HOME:-}" ]]; then
  echo "ANDROID_HOME is not set." >&2
  exit 1
fi

ADB="$ANDROID_HOME/platform-tools/adb"
EMULATOR="$ANDROID_HOME/emulator/emulator"

if [[ ! -x "$ADB" ]]; then
  echo "adb not found at $ADB" >&2
  exit 1
fi

if [[ ! -x "$EMULATOR" ]]; then
  echo "emulator not found at $EMULATOR" >&2
  exit 1
fi

if [[ ! -x "$JAVA_HOME/bin/java" ]]; then
  echo "java not found under JAVA_HOME=$JAVA_HOME" >&2
  exit 1
fi

if ! "$EMULATOR" -list-avds 2>/dev/null | grep -Fxq "$AVD_NAME"; then
  echo "AVD '$AVD_NAME' does not exist." >&2
  exit 1
fi

if ! "$ADB" devices | grep -q "^$EMULATOR_SERIAL[[:space:]]"; then
  echo "Starting emulator $AVD_NAME..."
  nohup "$EMULATOR" -avd "$AVD_NAME" -no-snapshot-save >/tmp/"$AVD_NAME".log 2>&1 &
fi

echo "Waiting for $EMULATOR_SERIAL..."
"$ADB" -s "$EMULATOR_SERIAL" wait-for-device
until [[ "$("$ADB" -s "$EMULATOR_SERIAL" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == "1" ]]; do
  sleep 2
done

export JAVA_HOME
export PATH="$JAVA_HOME/bin:$PATH"

cd "$APP_DIR"
exec npm run tauri android dev
