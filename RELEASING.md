# Publishing Bokheim APKs

GitHub Releases are the initial Android distribution channel. Releases contain
a signed ARM64 APK and its SHA-256 checksum. Google Play and AAB publication
are intentionally deferred.

## Permanent signing identity

The Android package is `se.bokheim.reader`. Every update must use the same
release signing key. Keep the keystore, alias, and passwords outside the
repository with at least two encrypted backups.

For local builds, copy
`MobileApp/src-tauri/gen/android/keystore.properties.example` to
`MobileApp/src-tauri/gen/android/keystore.properties` and enter the absolute
keystore path and credentials. The destination file and common keystore file
extensions are ignored by Git.

Build and verify a release locally:

```sh
cd MobileApp
JAVA_HOME=/usr/lib/jvm/java-21-openjdk CARGO_BUILD_JOBS=1 \
  npm run tauri -- android build --apk --target aarch64

$ANDROID_HOME/build-tools/36.0.0/apksigner verify --verbose --print-certs \
  src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk
```

## GitHub secrets

Configure these Actions secrets in the repository:

- `ANDROID_KEYSTORE_BASE64`: the release keystore encoded with `base64 -w 0`.
- `ANDROID_KEYSTORE_PASSWORD`: the keystore password.
- `ANDROID_KEY_ALIAS`: `bokheim` unless the local key used another alias.
- `ANDROID_KEY_PASSWORD`: the key password.

Do not place signing values in Actions variables, workflow files, issues, or
release notes.

## Release process

1. Update `MobileApp/src-tauri/tauri.conf.json` to the intended semantic version.
2. Run the local frontend, backend, binding, and signed-APK checks.
3. Commit and push the release changes.
4. Create and push an annotated matching tag, for example:

   ```sh
   git tag -a v0.1.0 -m "Bokheim 0.1.0"
   git push origin v0.1.0
   ```

5. The `Android APK release` workflow verifies the version, builds and checks
   the signed ARM64 APK, and creates a draft GitHub Release.
6. Download the workflow-built APK, install it on a clean phone, and complete a
   smoke test before manually publishing the draft.

The workflow can also be run manually. Manual runs retain the APK as a workflow
artifact but do not create a GitHub Release.
