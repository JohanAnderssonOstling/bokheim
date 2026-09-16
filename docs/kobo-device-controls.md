# Kobo brightness and connectivity

Investigated 2026-09-07. “Browser” here means Bokheim's native library browser.
The main reference is the local Plato checkout, commit
`922412057a194752a0df0dd325a8433f44c26333`.

## Implemented: brightness

`AppClient::kobo_brightness()` and `set_kobo_brightness(percent)` run through the
native backend's ordered worker queue. The `kobo` backend feature enables the
hardware adapter in `apps/kobo-device`. GPUI performs no device I/O during render.

Both EPUB and PDF reader toolbars inject a sun button in Kobo builds. Its popup
reads the driver when opened and provides a percentage, ±5 percentage-point
steps, and Off/10/25/50/75/100 presets. Requests disable the controls until
readback completes. Failures show an error and Retry rather than displaying an
unconfirmed value. Closing and reopening the popup refreshes the value.

The adapter uses `/sys/class/backlight/mxc_msp430.0/brightness`, the percentage
interface used by Plato's premixed frontlight driver, including Libra 2. It opens
the existing node for each write, rejects values above 100, and reads back the
result. Warmth is untouched. It does not write Nickel's preferences.

This first adapter supports the premixed intensity interface. Devices requiring
Plato's older `/dev/ntx_io` ioctl or individually mixed LED channels are not yet
supported; they report an unavailable driver. Supporting those models needs
model detection and separate adapters, not a generic write to any backlight node.

References:

- [Premixed driver](../reference/plato/crates/core/src/frontlight/premixed.rs)
- [Model selection](../reference/plato/crates/core/src/device.rs)
- [Older ioctl driver](../reference/plato/crates/core/src/frontlight/standard.rs)
- [Mixed LED driver](../reference/plato/crates/core/src/frontlight/natural.rs)
- [Frontlight popup](../reference/plato/crates/core/src/view/frontlight.rs)

Validation: three release hardware-adapter tests cover parsing, repeated writes,
Off, invalid values, and missing nodes. Release compilation covers the shared
EPUB/PDF toolbar. Actual frontlight changes and popup interaction still need a
Kobo check. Brightness persistence across application restarts and explicit
frontlight off/restore during suspend are separate follow-ups; Plato's
`PrepareSuspend` and `resume` in `crates/plato/src/app.rs` show the latter lifecycle.

## Wi-Fi findings

The follow-up [Wi-Fi investigation](kobo-wifi-investigation.md) contains the
concrete backend/browser design, lifecycle gaps, and a read-only device probe.
An initial [Wi-Fi implementation](kobo-wifi-implementation.md) now provides the
Kobo device top bar and connection popup; physical-device validation remains.

Plato's [enable script](../reference/plato/scripts/wifi-enable.sh) and
[disable script](../reference/plato/scripts/wifi-disable.sh) cover several
different drivers and power mechanisms:

| Driver family | Power and initialization |
| --- | --- |
| Most older modules | `sdio_wifi_pwr` plus the Wi-Fi module |
| `moal` | `/dev/ntx_io` request 208, `mlan`, then `moal`; `nl80211` |
| `wlan_drv_gen4m` | MediaTek WMT modules and `/dev/wmtWifi`; `nl80211` |

The scripts use `PLATFORM`, `WIFI_MODULE`, and `INTERFACE`, bring the network
interface up, start `wpa_supplicant`, and obtain an address with `udhcpc`.
They also account for firmware module locations and regulatory settings.
Plato's [launcher](../reference/plato/contrib/plato.sh) imports selected Nickel
environment variables before stopping Nickel. Its application restores Wi-Fi
after wake when enabled in its settings.

Plato's UI toggle starts these scripts. This is a reference for radio power and
reconnecting saved networks, not an implementation of a full network picker.
For scanning and joining, use the
[wpa_supplicant control interface](https://w1.fi/wpa_supplicant/devel/ctrl_iface_page.html):
status, scan/results, saved-network operations, and connection events. A Unix
control socket avoids interpolating SSIDs and credentials into shell commands.
Treat authentication and obtaining an IP address as distinct stages.

Our [launcher](../apps/desktop-gpui/kobo/run.sh) stops Nickel, but does not perform
Plato's environment capture or radio initialization. First record the actual
device's module, interface, supplicant control socket, and DHCP ownership. Then
implement a model-specific adapter and an idempotent radio state machine. Do not
blindly apply a script that terminates another process's DHCP client.

## Bluetooth findings

The local Plato checkout has no Bluetooth manager, pairing UI, or Bluetooth
enable/disable scripts. Its MediaTek Wi-Fi script loads a Bluetooth-related WMT
module; that is not evidence of a working pairing service.

The [kobo-btpt project](https://github.com/tsowell/kobo-btpt) demonstrates Bluetooth
page turning on Libra 2, but pairs through Kobo's existing settings and hooks
Nickel. Bokheim stops Nickel, so that integration cannot directly provide our UI.

First inspect the target firmware for `/sys/class/bluetooth/hci*`, its Bluetooth
daemon and version, D-Bus availability, controller initialization, and the input
profiles actually enabled. Do not assume that firmware exposes modern BlueZ.
If it exposes BlueZ 5, use the
[Adapter API](https://bluez.readthedocs.io/en/latest/adapter-api/) for power and
discovery, and the [Device API](https://bluez.readthedocs.io/en/latest/device-api/)
for pairing, connecting, and disconnecting. Pairing also needs an agent for
confirmation/passkey interaction. Firmware with another API needs a separate
adapter or a packaged daemon and controller bring-up.

Pairing is only part of page-turner support. GPUI's
`GPUI-Fork/crates/gpui_kobo/src/input.rs` currently discovers input sources at
startup and maps a small fixed set of key codes. Add hotplug/removal handling
and configurable remote key mappings so a remote connected after launch works.
Bluetooth audio additionally needs an audio profile and output routing; pairing
alone does not enable audiobook playback through headphones.

## Proposed backend and browser controls

Keep one application-owned device service and expose observed status separately
from requested state. UI switches must not claim success until hardware status
confirms it. Proposed API operations:

| Area | Operations and state |
| --- | --- |
| Capabilities | Supported frontlight adapter, Wi-Fi availability, Bluetooth availability |
| Wi-Fi | Power, status, scan, join, disconnect, forget; SSID, signal, authentication/IP stage, error |
| Bluetooth | Power, discovery, pair/cancel, connect/disconnect, forget; paired/connected devices and pairing prompts |
| Lifecycle | Prepare suspend, resume, restore desired state, refresh observed state |

Brightness sysfs operations are short and use the existing ordered worker path.
Radio initialization, scans, pairing and DHCP must run outside that synchronous
queue, with operation deadlines, cancellation, and status events, so they do not
block library requests or the UI. Serialize conflicting transitions within the
device service. Coordinate Wi-Fi and Bluetooth when they share power hardware.
Persist user choices separately from transient connected/powered state.

User requirement: put device controls in a new Kobo-only top bar above the
existing browser UI. Compose a `KoboDeviceBar` above `BrowserRoot` in
[`ApplicationRoot`](../apps/application-shell/src/lib.rs)'s browser branch,
gated by the `kobo` feature. Reserve space for the bar and let the existing
browser fill the remaining area. The bar stays visible across browser routes;
reader mode retains its existing brightness control.

Each device button opens its own popup below the bar, backed by the
application's device service:

- Brightness can reuse the implemented API immediately.
- Wi-Fi: switch and connection summary, then a network list with Refresh and
  a password prompt for secured networks. Show connection progress and Retry.
- Bluetooth: switch, saved devices, and Find devices. Device rows expose
  pair/connect/disconnect/forget, with passkey confirmation when required.
- Hide unsupported controls or show why the device cannot provide them.

The top bar's Wi-Fi and Bluetooth buttons reflect observed status even while
their popups are closed. Share that state with reader brightness controls
instead of creating separate hardware owners. Device controls are not placed
in the existing browser toolbar or browser settings section.

Implementation sequence: inspect Libra 2 firmware; support Wi-Fi power and saved
network reconnect; add scan/join UI; implement the detected Bluetooth transport;
add remote input hotplug/mapping; then verify both radios through sleep/wake.
Bluetooth controls and the full connectivity lifecycle remain proposals;
the initial Wi-Fi controls are described in the implementation status above.
