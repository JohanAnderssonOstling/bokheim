# Bokheim Kobo Installer

This small desktop application installs the release Kobo package onto a Kobo
Libra 2 connected as USB storage. The verified `tar.gz` package is embedded in
the Windows executable and Linux AppImage at build time; users do not select or
download a second payload.

The Linux AppImage uses the same glibc 2.17 release SDK and ELF checks as the
desktop app. Build it with `python3 scripts/linux/toolchain.py -- cargo build
--release --manifest-path apps/kobo-installer/Cargo.toml`, then run
`apps/kobo-installer/linux/build-appimage.sh`. See
[`../desktop-gpui/linux/README.md`](../desktop-gpui/linux/README.md) for SDK and
AppImage tool requirements.

The installer:

- attempts to mount an unmounted removable USB volume labeled `KOBOeReader`
  on Linux, then detects mounted volumes containing Kobo's `.kobo` marker directory;
- displays the exact mount path and requires an explicit **Install Bokheim**
  click;
- accepts package entries only below `.adds` and `Bokheim`;
- replaces individual files through temporary and backup files; and
- verifies that the launcher, FBInk, executable, PDFium runtime, and NickelMenu entry exist
  after extraction;
- queues the pinned NickelMenu v0.6.0 build with Bokheim direct-launch support; and
- attempts to safely eject the Kobo when installation finishes.

With a package supporting on-device updates, open **Settings → Kobo → Check
for updates** on the reader. Bokheim downloads the latest Kobo release over
Wi-Fi, verifies its published size and SHA-256 digest, and stages its files.
**Restart and install** closes the application before the launcher replaces
the program and starts it again. The packaged version must match the release
tag. Books, settings, databases, and optional SSH tools are excluded. This
flow keeps the existing NickelMenu installation and Kobo firmware.

A complete program-file backup is saved before replacement. If installation
is interrupted, the firmware shell restores it before the next launch. This
recovers installation failures; it does not roll back database migrations
after a successfully installed release. Older launchers need one USB install
of a package with on-device update support.

Tapping **Bokheim** in Kobo's bottom navigation opens Bokheim immediately.
The button uses Bokheim's logo, with gray and black versions for its normal
and active states. The installer embeds both icons even for older app packages.
The installer supplies a small NickelMenu extension and replaces older Bokheim
menu configuration. Configuration errors fall back to the regular menu.
Firmware before 4.23.15505 keeps the original menu behavior. See the
[extension build instructions](../desktop-gpui/kobo/nickelmenu-direct/README.md).

On the reader, **Settings → Kobo → Uninstall Bokheim** asks for confirmation,
then removes the program and its menu entry and returns to Kobo's built-in
reader. Books and settings are kept, as are other NickelMenu configurations
and separately installed developer SSH tools. This requires a Kobo package
built with uninstall support; older launchers report that a reinstall is needed.

NickelMenu installs when the Kobo disconnects and restarts. Firmware must be
4.6 or later in the 4.x series. An existing unrelated firmware update is
preserved and blocks installation until that update finishes.

Automatic eject uses `udisksctl` on Linux, `diskutil` on macOS, and volume
control calls on Windows. If host permissions or open files prevent eject,
the installer reports success and asks for manual safe eject. It does not
request elevated privileges. If only USB power-off fails after a successful
unmount, it reports that the device is safely unmounted.

On Linux, connection monitoring runs in the background and attempts mounting
once per connection using `udisksctl`. A permission failure asks the user to
mount manually. The monitor remembers mounted volumes so it cannot remount
the Kobo after eject. Windows and macOS use the operating system's normal
automatic mounting.

## Local development

Set `BOKHEIM_KOBO_ARCHIVE` to a package produced by
`apps/desktop-gpui/scripts/package-kobo.sh` and `BOKHEIM_VERSION` to the desktop
version before building the graphical executable.

```sh
BOKHEIM_KOBO_ARCHIVE=/absolute/path/to/desktop-gpui-kobo.tar.gz \
BOKHEIM_VERSION=0.1.0 \
cargo run --manifest-path apps/kobo-installer/Cargo.toml
```

The detection layer also accepts `BOKHEIM_KOBO_MOUNT` for development with a
temporary directory containing a `.kobo` directory.

After installing NickelMenu and enabling SSH separately, developers can deploy
over Wi-Fi while the Kobo is awake and Bokheim is closed:

```sh
BOKHEIM_KOBO_SSH_PORT=2222 apps/desktop-gpui/scripts/deploy-kobo-wifi.sh \
  root@192.168.1.123 /absolute/path/to/desktop-gpui-kobo.tar.gz
```

Use the server's actual port and an authorized SSH key. Trust its host key
through a normal SSH connection first. This script streams the package over
SSH, verifies its checksum and installed files, and preserves the one-entry
NickelMenu configuration. It requires neither SFTP nor a remote `scp` binary.

For a standalone development SSH server, build the pinned, statically linked
Dropbear with `sh apps/desktop-gpui/scripts/prepare-kobo-ssh.sh`. Copy the
resulting `target/desktop-gpui-kobo/ssh` directory into `.adds/bokheim/ssh` over
USB, and put the computer's SSH **public** key in `ssh/authorized_keys`. The
latest `run-monitored.sh` starts this optional server before launching Bokheim.
Create an empty `ssh/start-only` file to make the first tap start SSH while
leaving Nickel open; subsequent taps launch Bokheim normally. Eject and unplug
USB, enable Wi-Fi, then tap the existing Bokheim Kobo entry. This adds no menu
entries or firmware update. It must be started again after a reboot.

Dropbear listens on port 2222, with password authentication disabled at build
time. The bootstrap copies keys to `/etc/bokheim-ssh` with private permissions;
FAT storage cannot enforce SSH key permissions. It generates a device host key
there unless a unique, pre-provisioned `ssh/hostkey-seed` is supplied. That seed
is removed after installation. Verify the host fingerprint before trusting the
connection. Startup errors appear in `.adds/bokheim/ssh/start.log`; server logs
are in `/etc/bokheim-ssh/server.log`. Keep the Kobo awake for SSH access.
Set `BOKHEIM_KOBO_SSH_KEY=/path/to/private-key` for Wi-Fi deployment if the key
is not one of the SSH client's default identities.

To opt into persistent development SSH, copy the current `ssh/*.sh` scripts
alongside the provisioned Dropbear executable. Inspect `/proc/1/exe` and
`/etc/inittab` over SSH to confirm the Kobo uses BusyBox init, then run:

```sh
/bin/sh /mnt/onboard/.adds/bokheim/ssh/install-service.sh
```

Alternatively, with a configured and trusted SSH alias, install from the host:

```sh
python3 apps/desktop-gpui/scripts/install-kobo-ssh-service.py bokheim-kobo
```

The host helper inspects init, saves `/etc/inittab` locally, transfers and
verifies the scripts, and confirms init is supervising the service.

This explicitly changes `/etc/inittab`, adding one `respawn` action for
`/etc/bokheim-ssh/supervise.sh`. The original boot configuration is saved in
`/etc/bokheim-ssh/inittab.before-service`. The service keeps its executable and
keys on the internal filesystem, starts at boot, and restarts Dropbear if it
exits. It preserves an already-running server and its sessions during setup.
The launcher recognizes either server location. Service installation reloads
BusyBox init without rebooting; see the upstream [inittab format](https://github.com/mirror/busybox/blob/master/examples/inittab)
and [init implementation](https://github.com/mirror/busybox/blob/master/init/init.c).
Sleep and disabled Wi-Fi still make the reader unreachable. To disable boot
startup, remove the exact `::respawn:/bin/sh /etc/bokheim-ssh/supervise.sh` line
from `/etc/inittab` and reboot when convenient. This service is an explicit
developer setup step; the normal reader installer does not enable it.

## Releases

The Kobo package itself is built on a maintainer machine, not in CI. It uses
the pinned Bootlin ARMv7 glibc toolchain and bundles its runtime alongside
PDFium so the reader can load PDFium without replacing firmware libraries.
The built package is committed to the repository at
`package/Bokheim-Kobo-armv7.tar.gz`, and the `Kobo release` workflow
embeds whatever is committed there.

Whenever the Kobo application changes, rebuild and commit the package before
running the workflow:

```sh
apps/desktop-gpui/scripts/refresh-kobo-package.sh
git add apps/kobo-installer/package/Bokheim-Kobo-armv7.tar.gz
git commit -m "Refresh the committed Kobo package"
```

The script cross-compiles the package, applies the checks listed above, and
replaces the committed archive only once they pass. Note that each refresh
adds a new copy of the archive to the repository's history permanently.
