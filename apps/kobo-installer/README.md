# Bokheim Kobo Installer

This small desktop application installs the release Kobo package onto a Kobo
Libra 2 connected as USB storage. The verified `tar.gz` package is embedded in
the Windows executable and Linux AppImage at build time; users do not select or
download a second payload.

The installer:

- detects mounted volumes containing Kobo's `.kobo` marker directory;
- displays the exact mount path and requires an explicit **Install Bokheim**
  click;
- accepts package entries only below `.adds` and `Bokheim`;
- replaces individual files through temporary and backup files; and
- verifies that the launcher, FBInk, executable, and NickelMenu entry exist
  after extraction.

NickelMenu is a separate prerequisite. The installer reports whether its
configuration directory was detected, but it does not modify Kobo's system
partition or install third-party firmware hooks.

## Local development

Set `BOKHEIM_KOBO_ARCHIVE` to a package produced by
`apps/desktop-gpui/scripts/package-kobo.sh` and `BOKHEIM_VERSION` to the desktop
version before building the graphical executable.

```sh
BOKHEIM_KOBO_ARCHIVE=/absolute/path/to/desktop-gpui-kobo.tar.gz \
BOKHEIM_VERSION=0.1.6 \
cargo run --manifest-path apps/kobo-installer/Cargo.toml
```

The detection layer also accepts `BOKHEIM_KOBO_MOUNT` for development with a
temporary directory containing a `.kobo` directory.

## Releases

The Kobo package itself is built on a maintainer machine, not in CI: its ARMv7
musl cross toolchain is published by a single volunteer-run host that is
regularly unreachable, which is not acceptable in the release path. Publish it
first, then build the installers against it:

```sh
apps/desktop-gpui/scripts/publish-kobo-release.sh   # --dry-run to verify only
```

The script refuses to publish from a dirty tree or an unpushed commit, applies
the package checks listed above, and uploads the archive and its checksum to
the shared draft release. The `Kobo release` workflow then downloads that
package, verifies its checksum, and embeds it in the Windows and Linux
installers.
