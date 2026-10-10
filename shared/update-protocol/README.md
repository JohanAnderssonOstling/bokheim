# Bokheim update protocol

Shared signed release manifests and artifact verification for clients and the update publisher.
The crate has no HTTP, installation, or durable-state dependency. Client discovery and
activation live in [`client/update-client`](../../client/update-client).

## Protocol v1

Serve JSON in an envelope:

```json
{"key_id":"release-1","payload":"<exact JSON text>","signature":"<128 hexadecimal characters>"}
```

The Ed25519 signature covers the bytes `bokheim-update-manifest-v1`, one NUL byte,
and the exact UTF-8 bytes of the decoded `payload` string, in that order. Sign the
original string; do not reserialize it before verifying. Envelope size is capped
at 1 MiB. Unknown fields are rejected in v1 to prevent silently ignoring new
requirements; incompatible changes need a new protocol version.

Illustrative payload (replace dates, sizes, hashes, and URLs before publishing):

```json
{
  "protocol_version": 1,
  "channel": "stable",
  "sequence": 42,
  "issued_at_unix": 1790553600,
  "expires_at_unix": 1791158400,
  "application": {
    "version": "0.1.7",
    "name": "Bokheim 0.1.7",
    "schema_version": 75,
    "taxonomy_format_versions": [5],
    "artifacts": {
      "android-aarch64-apk": {
        "url": "https://downloads.bokheim.se/releases/0.1.7/android-arm64.apk",
        "bytes": 123456,
        "sha256": "<64 hexadecimal characters>"
      }
    }
  },
  "taxonomy": {
    "release_id": 12,
    "format_version": 5,
    "minimum_application_version": "0.1.7",
    "minimum_schema_version": 75,
    "artifact": {
      "url": "https://taxonomy.bokheim.se/releases/12/snapshot.sqlite3",
      "bytes": 234567,
      "sha256": "<64 hexadecimal characters>"
    }
  }
}
```

Both `application` and `taxonomy` may be null. Remove a withdrawn offer and publish
a higher sequence. Platform package IDs used by the desktop adapters are
`linux-x86_64-appimage` and `windows-x86_64-installer`. The contract supports other
IDs without coupling them to a hosting provider. Android APK signature continuity
and versionCode still require installer validation; this manifest is not a
replacement for Android package identity checks.

`offer` evaluates taxonomy against the selected application when an application
upgrade exists for the current target, or against installed capabilities otherwise.
The activation coordinator must additionally validate the actual migration path
and compatibility of retained taxonomy before approving a plan. These fields are
not permission to run downloaded migration code.

## Publishing

1. Build optimized release artifacts and validate them.
2. Upload immutable version-specific artifacts to any HTTPS host. Redirects from
   Bokheim-owned URLs may point to replaceable storage providers.
3. Populate byte lengths and SHA-256 digests from the actual uploaded artifacts.
4. Allocate an increasing sequence and bounded publication lifetime.
5. Sign and validate locally using a protected Ed25519 PKCS#8 private key:

   ```sh
   cargo run --release -p update-protocol --example sign_manifest -- manifest.json /secure/release-key.pk8 release-1 signed.json
   ```

6. Publish the signed envelope atomically only after its artifacts are available.
   Refresh expiration through a newly signed, higher-sequence publication. Keep
   pinned artifact URLs available for previously approved downloads.

GitHub may host files or run builds; clients neither call its release API nor
interpret its release names or asset filenames. Existing CI artifact publication
is unchanged; publishing the signed feed is a separate deployment step.

## Validation

```sh
cargo test -p update-protocol
cargo check -p update-protocol --target wasm32-unknown-unknown
```

## Hosting: static files are sufficient

No database or new application service is required for discovery. An existing
HTTPS web server or static object host can serve the signed JSON envelope at
`/v1/stable`. Serve manifest responses with `Content-Type: application/json` and
cache revalidation (ETag or Last-Modified, with a short cache lifetime). Publish
versioned artifacts with immutable URLs and long-lived caching. Enable public,
credential-free CORS for manifest and artifact requests if browser clients fetch
them across origins.

The publisher owns release selection, signing, and atomic publication. The host
only serves bytes; it never needs the private signing key. Use the existing Bokheim
HTTPS infrastructure if convenient. A separate `updates.bokheim.se` hostname makes
it possible to move the static host without changing clients. Provisioning DNS/TLS
and deploying signed files is still required before this proposed endpoint works.

