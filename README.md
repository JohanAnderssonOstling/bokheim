# Bokheim

Bokheim is a local-first EPUB library and reader with shared Rust library and
synchronization logic, desktop frontends, a server, and a responsive Tauri 2
mobile application powered by Svelte and Foliate.js.

The Android application identifier is `se.bokheim.reader`. It supports Android
7.0 (API 24) and later.

## Repository layout

- `Backend`: shared library, persistence, reading state, downloads, and sync.
- `MobileApp`: responsive Android application and Foliate.js reader.
- `Server`: synchronization service.
- `HtmlRenderer`: native HTML/EPUB rendering crates and host adapters.
- `UI` and `GPUI`: desktop application frontends.
- `SyncCommon`: shared synchronization protocol types.

Clone the repository with submodules:

```sh
git clone --recurse-submodules https://github.com/JohanAnderssonOstling/HTML-prototype.git
```

Mobile development and validation commands are documented in
[`MobileApp/README.md`](MobileApp/README.md). Android publication is documented
in [`RELEASING.md`](RELEASING.md).

## Privacy and licensing

See [`PRIVACY.md`](PRIVACY.md), [`LICENSE`](LICENSE), and
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
