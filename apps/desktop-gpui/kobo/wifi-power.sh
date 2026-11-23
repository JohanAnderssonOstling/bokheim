#!/bin/sh
# Kobo radio adapter. Nickel does not export driver hints on every firmware.
set -eu
step='validate hardware hints'
trap 'status=$?; if [ "$status" -ne 0 ]; then printf "Wi-Fi helper failed at %s (status=%s)\n" "$step" "$status" >&2; fi' 0
loaded() { awk -v module="$1" '$1 == module { found=1 } END { exit !found }' /proc/modules; }
module_state="$(dirname "$0")/wifi-module"
if [ -z "${WIFI_MODULE:-}" ]; then
    # Prefer the running kernel over a saved hint. Do not guess from installed
    # .ko files: Kobo firmware can ship several different Wi-Fi drivers.
    for candidate in dhd 8189fs 8189es 8723bs 8723ds 8821cs moal; do
        if loaded "$candidate"; then
            [ -z "${WIFI_MODULE:-}" ] || { printf 'Multiple Wi-Fi drivers are loaded\n' >&2; exit 2; }
            WIFI_MODULE=$candidate
        fi
    done
    # Shutdown unloads the driver. Retain its name for subsequent wake cycles
    # even when the application was launched without WIFI_MODULE.
    if [ -z "${WIFI_MODULE:-}" ] && [ -r "$module_state" ]; then
        IFS= read -r WIFI_MODULE < "$module_state" || true
    fi
fi
case "${WIFI_MODULE:-}" in dhd|8189fs|8189es|8723bs|8723ds|8821cs|moal) ;; *) printf 'Missing or unsupported Wi-Fi driver: %s\n' "${WIFI_MODULE:-unset}" >&2; exit 2 ;; esac
if [ "${1:-}" = detect ]; then printf '%s\n' "$WIFI_MODULE"; exit 0; fi
case "${INTERFACE:-}" in ''|-*|.*|*[!a-zA-Z0-9_.-]*) printf 'Missing or invalid Wi-Fi interface\n' >&2; exit 2 ;; esac
case "${PLATFORM:-}" in -*|.*|*[!a-zA-Z0-9_.-]*) printf 'Invalid Wi-Fi platform\n' >&2; exit 2 ;; esac
case "${1:-}" in on|radio-on|off|off-wifi|handoff) ;; *) exit 2 ;; esac
printf '%s\n' "$WIFI_MODULE" > "$module_state"
resolve_module() {
    # PLATFORM is not exported by every Nickel launcher. Shutdown does not
    # need it; startup can locate a unique installed driver instead.
    module_file=''
    if [ -n "${PLATFORM:-}" ]; then
        for candidate in "/drivers/$PLATFORM/$WIFI_MODULE.ko" "/drivers/$PLATFORM/wifi/$WIFI_MODULE.ko"; do
            if [ -f "$candidate" ]; then module_file=$candidate; break; fi
        done
    else
        for candidate in /drivers/*/"$WIFI_MODULE.ko" /drivers/*/wifi/"$WIFI_MODULE.ko"; do
            [ -f "$candidate" ] || continue
            [ -z "$module_file" ] || { printf 'Multiple Wi-Fi driver paths; PLATFORM is required\n' >&2; return 3; }
            module_file=$candidate
        done
    fi
    [ -n "$module_file" ] || { printf 'Wi-Fi driver file is missing\n' >&2; return 3; }
    module_dir=$(dirname "$module_file")
    [ ! -d "$module_dir/wifi" ] || module_dir="$module_dir/wifi"
}
remove_module() {
    # Supplicant termination is asynchronous; the driver can remain busy briefly.
    remaining=20
    while loaded "$1"; do
        if rmmod "$1"; then return 0; fi
        remaining=$((remaining-1))
        [ "$remaining" -gt 0 ] || return 1
        sleep 0.2
    done
}
stop_processes() {
    for service_pid in $1; do
        kill -TERM "$service_pid" 2>/dev/null || true
    done
    remaining=30
    while :; do
        service_alive=''
        for service_pid in $1; do
            if IFS= read -r service_stat 2>/dev/null < "/proc/$service_pid/stat"; then
                case "${service_stat##*) }" in
                    Z\ *|X\ *) ;;
                    *) service_alive="$service_alive $service_pid" ;;
                esac
            fi
        done
        [ -n "$service_alive" ] || return 0
        remaining=$((remaining-1))
        [ "$remaining" -gt 0 ] || return 1
        sleep 0.1
    done
}
ntx_power() {
    # Use the app's libc adapter instead of depending on a firmware ioctl utility.
    "$(dirname "$0")/desktop-gpui-kobo" --wifi-power-ioctl "$1"
}
stop_bluetooth() {
    # The Libra 2's UART attach process must not retain a controller backed by
    # a powered-down RTL8723DS. BlueZ and its pairing cache stay available.
    [ "$WIFI_MODULE" = 8723ds ] || return 0
    if [ -d /sys/class/bluetooth/hci0 ]; then
        # Stay in the caller's bounded process group. A nested isolated helper
        # could otherwise escape when the entire radio operation is cancelled.
        bluetoothctl --timeout 2 power off >/dev/null 2>&1 || true
        /bin/hciconfig hci0 down >/dev/null 2>&1 || true
    fi
    attach_pids=''
    for service_pid in $(pidof rtk_hciattach 2>/dev/null || true); do
        if tr '\000' '\n' < "/proc/$service_pid/cmdline" 2>/dev/null | grep -Fxq /dev/ttymxc1; then
            attach_pids="$attach_pids $service_pid"
        fi
    done
    stop_processes "$attach_pids"
}
case "${1:-}" in
    on|radio-on)
        step="start radio"
        if ! loaded "$WIFI_MODULE"; then
            resolve_module
            if [ "$WIFI_MODULE" = moal ]; then
                ntx_power 1
            else
                loaded sdio_wifi_pwr || insmod "$module_dir/sdio_wifi_pwr.ko"
            fi
            country=$(sed -n 's/^WifiRegulatoryDomain=//p' '/mnt/onboard/.kobo/Kobo/Kobo eReader.conf' 2>/dev/null | head -n 1)
            case "$country" in [A-Z][A-Z]) ;; *) country='' ;; esac
            if [ "$WIFI_MODULE" = moal ]; then
                dep="$module_dir/mlan.ko"
                [ -f "$dep" ] || dep="$(dirname "$module_dir")/mlan.ko"
                loaded mlan || insmod "$dep"
                if [ -n "$country" ]; then
                    insmod "$module_file" mod_para=nxp/wifi_mod_para_sd8987.conf "reg_alpha2=$country"
                else
                    insmod "$module_file" mod_para=nxp/wifi_mod_para_sd8987.conf
                fi
            elif [ "$WIFI_MODULE" = 8821cs ] && [ -n "$country" ]; then
                insmod "$module_file" "rtw_country_code=$country"
            else
                insmod "$module_file"
            fi
        fi
        remaining=20
        while [ ! -d "/sys/class/net/$INTERFACE" ]; do
            [ "$remaining" -gt 0 ] || exit 4
            remaining=$((remaining-1))
            sleep 0.2
        done
        # Bluetooth needs the module powered, but must not enable Wi-Fi when
        # the user had disabled it before initializing the Bluetooth adapter.
        [ "$1" != radio-on ] || exit 0
        ifconfig "$INTERFACE" up
        [ "$WIFI_MODULE" != dhd ] || wlarm_le -i "$INTERFACE" up
        if ! wpa_cli -i "$INTERFACE" ping 2>/dev/null | grep -q '^PONG$'; then
            driver=wext
            [ "$WIFI_MODULE" != moal ] || driver=nl80211
            env -u LD_LIBRARY_PATH wpa_supplicant -B -D "$driver" -i "$INTERFACE" \
                -c /etc/wpa_supplicant/wpa_supplicant.conf -C /var/run/wpa_supplicant
        fi
        ;;
    off|off-wifi|handoff)
        if [ "$1" = handoff ]; then
            # Nickel owns this bridge. Its old connection refers to the
            # supplicant/driver that Bokheim may have replaced during sleep.
            step="stop firmware Wi-Fi bridge"
            stop_processes "$(pidof dhcpcd-dbus 2>/dev/null || true)"
            # A DHCP request can outlive an app terminated by a signal.
            dhcp_pids=''
            for service_pid in $(pidof udhcpc 2>/dev/null || true); do
                if tr '\000' '\n' < "/proc/$service_pid/cmdline" 2>/dev/null |
                    awk -v interface="$INTERFACE" '
                        previous == "-i" && $0 == interface { found=1 }
                        $0 == "-i" interface { found=1 }
                        { previous=$0 }
                        END { exit !found }
                    '; then
                    dhcp_pids="$dhcp_pids $service_pid"
                fi
            done
            stop_processes "$dhcp_pids"
        fi
        step="release $INTERFACE address"
        # Use the firmware client's interface-specific shutdown. Leaving it
        # running while unloading the driver can race subsequent address setup.
        if command -v dhcpcd >/dev/null 2>&1; then
            dhcpcd -k "$INTERFACE" >/dev/null 2>&1 || true
        fi
        step="stop supplicant and bring $INTERFACE down"
        if [ -d "/sys/class/net/$INTERFACE" ]; then
            wpa_cli -i "$INTERFACE" terminate >/dev/null 2>&1 || true
            [ "$WIFI_MODULE" != dhd ] || wlarm_le -i "$INTERFACE" down
            ifconfig "$INTERFACE" down
        fi
        # Keep the shared radio powered while Bluetooth is in use.
        [ "$1" != off-wifi ] || exit 0
        step="release shared Bluetooth controller"
        stop_bluetooth
        step="unload $WIFI_MODULE"
        remove_module "$WIFI_MODULE"
        if [ "$WIFI_MODULE" = moal ]; then
            step="unload mlan"
            remove_module mlan
        fi
        step="switch radio power off"
        # Match Plato's driver-specific power method. An already unloaded
        # power module is already off; it must not trigger an unrelated ioctl.
        if [ "$WIFI_MODULE" = moal ]; then ntx_power 0; else remove_module sdio_wifi_pwr; fi
        ;;
    *) exit 2 ;;
esac
