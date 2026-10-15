# Bokheim direct launch

This extension builds on NickelMenu v0.6.0, commit
`15a95f7e6aa32b053094b062a4157245aecb4cba`, and its pinned NickelHook submodule.
The bottom navigation button runs the main-menu item named by
`experimental:menu_main_15505_direct:Bokheim Kobo` immediately. It uses
NickelMenu's existing action execution and error display. Invalid configuration,
a missing label, or duplicate matching main-menu labels retain the normal menu.
Removing the setting restores the menu, including other applications' entries.
Firmware before 4.23.15505 retains its original menu behavior.

Build the payload with NickelTC 1.0 on `PATH`, or specify its extracted root:

```sh
NICKELTC_ROOT=/path/to/arm-nickel-linux-gnueabihf \
  bash apps/desktop-gpui/scripts/build-nickelmenu.sh
```

The script pins the source revision, applies `direct-launch.patch`, copies
`direct-launch.h`, and builds and strips the ARM plugin. It creates the
`nickelmenu-install/KoboRoot.tgz` payload with fixed archive timestamps and
updates both installer checksums and `VERSION.txt`. The desktop installer embeds
this payload independently of the larger Bokheim application archive.

The toolchain used for this build is `ghcr.io/pgaskin/nickeltc:1.0`, manifest
`sha256:3296eb400d270cfd26f5773ab09625d4469f85680b3a191951dfb78d8ade7840`.
The toolchain layer has SHA256
`862464e9e2622637c11fb49c036a98e99998c1328b130422a1dea6d4b45c9e21`.
See [NickelTC](https://github.com/pgaskin/NickelTC) for extraction instructions.
NickelMenu and NickelHook remain MIT licensed; the existing license accompanies
the installer payload.
