# Android reader

Build an optimized ARM64 release APK with a JDK supported by Gradle (for example JDK 21),
Android SDK 36, the configured Android NDK, and Rust's `aarch64-linux-android`
target installed:

```sh
gradle assembleRelease -Prust.ndkHome=/path/to/android/ndk
adb install -r build/outputs/apk/release/BokheimGpui-release.apk
```

Run these commands in this directory. `ANDROID_HOME` must point to the SDK.
On a memory-constrained build machine, set `CARGO_BUILD_JOBS=2` and pass
`--max-workers=2` to Gradle.

## Audiobooks

The Android feature enables the shared audiobook reader. Open/import an M4B from
the library or an Android document provider. Android uses Media3/ExoPlayer for
decoding and playback; desktop builds retain Rodio and WSOLA. Audio codec support
on Android follows the device's Media3/platform decoders (AAC M4B is the primary
format).

`AudiobookService` owns the player and media session. Playback continues when the
screen turns off or another app opens. The notification/lock screen and headset
buttons control this same session. Audio focus and headphone disconnection are
handled by Media3. The reader's close button explicitly stops playback.

The service sends position updates to Rust independently of the UI. Positions
are saved about every five seconds of progress and at pauses, seeks, and service
shutdown. Reopening a book uses its saved library position. Force-stopping the
process can lose the most recent unsaved interval.

Local audiobooks use an open file descriptor. Remote audiobooks use Media3 byte
reads backed by the backend's authenticated, seekable range reader, so opening
and seeking do not require a complete download or temporary playback file.
The service retains the source while background playback is active.

## Validation

```sh
cargo test --release -p audiobook-player --lib
gradle testReleaseUnitTest -x buildRustRelease
```

The Rust tests cover background persistence, stale seek events, source lifetime,
and the shared desktop playback/chapter logic. Robolectric service tests cover
restoring an initial position, metadata, seek/speed commands, replacement sessions,
cold-start commands, explicit stop, and rejection of file paths in external
service intents. Unit tests do not establish audible playback or device-specific
background behavior.

On a phone, verify an imported AAC M4B plays, seeks to chapters, changes speed
without changing pitch, continues with the screen locked, responds to notification
and headset controls, pauses on headphone disconnection/interruption, and resumes
at its saved position after closing and reopening the app.

## Native account screen and password managers

Settings opens `AccountDialog`, with native Android email/password fields for
sign-in, registration, verification, and password recovery. Requests cross JNI
to the existing Rust account client. Failed requests retain the fields;
successful sign-in commits the Autofill Framework session. Registration and
reset responses do not prove the displayed email/password pair, so those
sessions are cancelled; save confirmation waits for a successful sign-in.
Closing the dialog cancels autofill, and session IDs reject stale responses.
Credentials are not placed in Android saved-instance state.

The app declares a credential association with `https://app.bokheim.se`.
Before publishing that association, choose the production signing certificate.
The current Gradle release configuration still uses the local debug key; do not
publish that certificate as the production website's trusted identity. For Play
App Signing, use the app signing certificate, not the upload certificate.

From the repository root, generate the website file using its public SHA-256
fingerprint (repeat `--sha256` for multiple production signing certificates):

```sh
python3 apps/desktop-gpui/android/scripts/generate-assetlinks.py \
  --sha256 "$BOKHEIM_ANDROID_CERT_SHA256" \
  --output apps/web-gpui/assetlinks.json
```

Keep the generated file with the deployment sources. The web release build
copies it to `.well-known/assetlinks.json`. Deploy the Caddy route as well as the
web assets, then verify the HTTPS endpoint returns 200 and JSON without a
redirect. Until a certificate is configured, the endpoint should return 404.
This association declares credential sharing; it does not enable URL handling.

Run the native form checks using the release variant:

```sh
gradle -p apps/desktop-gpui/android testReleaseUnitTest \
  --tests se.bokheim.reader.gpui.AccountDialogTest -x buildRustRelease
```

The tests cover values filled into native fields, hints, pending requests,
errors, registration/verification/reset, successful commits, and dismissal.
They do not establish actual password-manager suggestions or save prompts.
On a device with Bitwarden selected as the autofill provider, test a vault entry
containing only `https://app.bokheim.se`, then filling, saving, failed login,
password updates, Back, rotation, and closing/reopening with an empty password.
Provider support for website/app associations must be verified; a provider may
still require the Android app URI on the same vault entry.

## PDFium

Gradle prepares checksum-verified Android PDFium through
`shared/pdfium/prepare.py` before compiling Rust, then packages `libpdfium.so`
under `lib/arm64-v8a/` in the APK. Python 3.11 or newer is required.
The Android feature enables PDF reading, search, selection, and cover extraction.
License notices are included in the shared reader. No PDFium source compilation
or runtime extraction is required. See `shared/pdfium/README.md` for offline setup.
