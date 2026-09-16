# Initial Kobo Wi-Fi implementation

The native Kobo browser now has its own device top bar above the existing
browser UI. Its Wi-Fi button opens a popup. The bar is compiled only for Kobo
and is shown only while browsing; the reader keeps its own toolbar.

## Available controls

- Observed connection status and current network.
- Radio on/off through the packaged firmware adapter.
- Refresh nearby networks, with signal level and security type.
- Connect to networks already configured in the running supplicant.
- Join open and WPA-Personal networks with password entry.
- Disconnect or cancel an in-progress connection.

New network configurations last for the active supplicant session. This version
does not persist new credentials or edit Nickel's saved-network configuration.
Turning the radio off and on restarts the supplicant and reloads firmware-saved
networks. Unsupported security modes are displayed but cannot be selected.
Hidden-network entry, persistent Forget/Remember, Bluetooth controls, and the
battery indicator remain follow-up work.

## Implementation

`AppClient::kobo_wifi(WifiRequest)` exposes the native API. A dedicated
`kobo-wifi` worker in `kobo-device` serializes device operations independently
of the application/library worker. The popup uses this API; it performs no
network or filesystem operations while rendering.

The supplicant client uses `wpa-ctrl` 0.4.0 with private Unix-domain sockets and
a separate event monitor. The vendored dependency has small transport fixes
documented in [LOCAL-PATCHES.md](../vendor/wpa-ctrl/LOCAL-PATCHES.md). Scan completion comes from supplicant events. Commands have read/write
timeouts and oversized replies are rejected. Passwords are converted to a PSK
and sent through the control socket; neither command lines nor errors contain
credentials. Closing the popup clears the password input.

Network identity retains SSID bytes, including non-UTF-8 bytes, while the UI
uses a display string. Scan grouping preserves different security modes and
keeps the strongest observed signal. Cancellation restores the prior enabled
network set. A failed new-network configuration removes its temporary entry.
Authentication and obtaining an address are separate states with timeouts.

The UI refreshes status every two seconds while open/connecting and every
30 seconds otherwise. Background connection work continues independently of
the popup. Unchanged status does not trigger extra UI notifications. Scans are
user initiated and bounded; closing the popup does not forcibly abort a kernel
scan already in progress.

`run.sh` captures only selected hardware hints from Nickel before stopping it.
`wifi-power.sh` is included in the release package. It handles the enumerated
legacy/Realtek driver families and `moal`, with a power module or request 208
through `/dev/ntx_io`, following the interfaces documented by the local Plato
checkout. MediaTek WMT is explicitly unsupported in this adapter. The helper
preserves supported firmware regulatory settings and validates hardware hints.

## Sleep

After the sleep page is rendered, `kobo-device::suspend` synchronously asks the
Wi-Fi worker to stop its DHCP process and run the radio-off helper. Only successful
shutdown permits touch deactivation and kernel suspend. A failed or unsupported
shutdown aborts suspend and reports the error. An unresponsive supplicant does not
prevent the power-off helper from running. Sleep preparation has a 20-second total deadline, including queue wait. Expired
queued requests cannot begin radio shutdown. Helper execution is bounded to 15
seconds (or the remaining sleep deadline); on timeout its process group is killed.
Resetting an old IP address is bounded to five seconds. A late worker reply cannot
leave the sleep guard active after the caller has aborted sleep.

A guard blocks scans, joins and power-on requests until suspend and hardware recovery
finish, including error exits. Once GPUI presents the first restored application
frame, its `wake_ui_ready_handler` queues Wi-Fi restoration on the device worker.
Radio startup and DHCP run in the background. Wi-Fi is restored only if it was on
before sleep; explicit user actions override a pending restore. Duplicate callbacks
do not restart it, and startup failures appear in the Wi-Fi status. Because new
credentials remain session-only, sleeping also discards newly joined networks
from the terminated supplicant session.

## Validation and remaining device work

Twenty-three release tests pass, covering brightness and Wi-Fi. Wi-Fi tests use a fake
supplicant socket to verify request/response handling, event delivery,
credential-safe errors, failed-join cleanup, cancellation, SSID handling and
authentication/address state, response whitespace, command bounds, request timeouts,
sleep shutdown, deferred restoration, user overrides, startup failures, service
loss cleanup, sleep reply timeouts and helper process cancellation. The device crate passes an ARM release check.
The full Kobo application also passes its release check.
Launcher/helper shell syntax and the embedded SVG were checked previously.

No package has been deployed and no radio has been toggled on a physical Kobo.
The target firmware must still be checked with `inspect-wifi.sh`: driver power,
existing DHCP ownership, address acquisition, and handoff to Nickel need device
validation. Existing firmware DHCP managers may require a dedicated handoff
adapter. Deferred Wi-Fi restoration after wake also needs device validation. See the
[investigation](kobo-wifi-investigation.md) for that lifecycle design.
