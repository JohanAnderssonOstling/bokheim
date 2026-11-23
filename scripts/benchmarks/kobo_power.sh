#!/bin/sh
# Run on the Kobo. Records battery-gauge readings in RAM without an SSH session.
# Usage: sh kobo_power.sh inspect
#        sh kobo_power.sh record bokheim-idle 900 10 /tmp/bokheim-idle.tsv
# Use matching labels nickel-idle, bokheim-reading, and nickel-reading.
# Keep lighting, wireless settings, book and page-turn cadence the same.
# Disconnect charging. Periodic sampling is intended for awake measurements.
set -eu

find_battery() {
    for supply_name in bd71827_bat mc13892_bat battery; do
        if [ -r "/sys/class/power_supply/$supply_name/capacity" ]; then
            battery_path="/sys/class/power_supply/$supply_name"
            return
        fi
    done
    for supply_path in /sys/class/power_supply/*; do
        supply_type=''
        if [ -r "$supply_path/type" ]; then
            IFS= read -r supply_type < "$supply_path/type" || true
        fi
        if [ "$supply_type" = Battery ]; then
            battery_path=$supply_path
            return
        fi
    done
    echo 'No readable battery gauge found' >&2
    exit 1
}

read_attribute() {
    attribute_value=NA
    if [ -r "$battery_path/$1" ]; then
        IFS= read -r attribute_value < "$battery_path/$1" || attribute_value=NA
    fi
}

find_battery
battery_device=$(readlink -f "$battery_path/device")
current_now_semantics=current_ua
case "$battery_device" in
    */ricoh619-battery)
        # Kobo's Libra 2 vendor driver returns RE_CAP (remaining mAh) here,
        # rather than current. Preserve the raw value and normalize charge_now.
        current_now_semantics=remaining_charge_mah
        ;;
esac
case "${1:-inspect}" in
    inspect)
        echo "Battery: $battery_path"
        echo "current_now semantics: $current_now_semantics"
        cat "$battery_path/uevent"
        exit
        ;;
    record) ;;
    *) echo 'Usage: kobo_power.sh inspect | record LABEL SECONDS INTERVAL OUTPUT' >&2; exit 2 ;;
esac

label=${2:?Supply an app/workload label}
duration=${3:-900}
interval=${4:-10}
output=${5:-/tmp/kobo-power-$$.tsv}
case "$label" in
    bokheim-idle|nickel-idle|bokheim-reading|nickel-reading) ;;
    *) echo 'Label must be bokheim-idle, nickel-idle, bokheim-reading or nickel-reading' >&2; exit 2 ;;
esac
for number in "$duration" "$interval"; do
    case "$number" in ''|*[!0-9]*|0*) echo 'Duration and interval must be positive integers' >&2; exit 2 ;; esac
done
case "$output" in /tmp/*.tsv) ;; *) echo 'Write to /tmp/*.tsv to keep sampling off flash storage' >&2; exit 2 ;; esac
read_attribute status
if [ "$attribute_value" != Discharging ]; then
    echo "Battery must report Discharging; currently: $attribute_value" >&2
    exit 1
fi

# Refuse to overwrite an earlier measurement.
set -C
exec 3> "$output"
set +C
printf '# label=%s\n# battery=%s\n' "$label" "$battery_path" >&3
printf '# current_now_semantics=%s\n' "$current_now_semantics" >&3
printf '# Units assume the Linux power-supply ABI: uV, uA, uAh, uWh.\n' >&3
printf '# Gauge accuracy and refresh rate must be checked on the device.\n' >&3
printf '# kernel=%s\n' "$(uname -r)" >&3
printf 'uptime_s\tstatus\tcapacity\tvoltage_now\tcurrent_now\tcurrent_avg\tcharge_now\tenergy_now\n' >&3
{
    date -u
    echo "label=$label duration=$duration interval=$interval battery=$battery_path"
    cat "$battery_path/uevent"
    ps
    cat /proc/net/wireless
    for setting in /sys/class/backlight/*/brightness /sys/class/backlight/*/color \
        /sys/class/leds/*/brightness /sys/class/leds/*/color \
        /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor; do
        if [ -r "$setting" ]; then
            echo "$setting"
            cat "$setting"
        fi
    done
    if [ -r /mnt/onboard/.adds/bokheim/BUILD-INFO.txt ]; then
        cat /mnt/onboard/.adds/bokheim/BUILD-INFO.txt
    fi
} > "$output.metadata"

read -r started_uptime _ < /proc/uptime
started_seconds=${started_uptime%.*}
while :; do
    read -r sample_uptime _ < /proc/uptime
    printf '%s' "$sample_uptime" >&3
    sample_status=NA
    for attribute in status capacity voltage_now current_now current_avg charge_now energy_now; do
        read_attribute "$attribute"
        if [ "$attribute" = charge_now ] && [ "$attribute_value" = NA ] \
            && [ "$current_now_semantics" = remaining_charge_mah ]; then
            read_attribute current_now
            case "$attribute_value" in
                ''|*[!0-9]*) attribute_value=NA ;;
                *) attribute_value=$((attribute_value * 1000)) ;;
            esac
        fi
        printf '\t%s' "$attribute_value" >&3
        if [ "$attribute" = status ]; then sample_status=$attribute_value; fi
    done
    printf '\n' >&3
    if [ "$sample_status" != Discharging ]; then
        echo "Stopped: battery status changed to $sample_status" >&2
        exit 1
    fi
    elapsed=$((${sample_uptime%.*} - started_seconds))
    if [ "$elapsed" -ge "$duration" ]; then break; fi
    remaining=$((duration - elapsed))
    pause=$interval
    if [ "$remaining" -lt "$pause" ]; then pause=$remaining; fi
    sleep "$pause"
done
printf 'Saved %s\n' "$output"
