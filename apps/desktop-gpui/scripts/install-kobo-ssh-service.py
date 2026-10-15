#!/usr/bin/env python3
"""Install the opt-in SSH boot service through an already trusted SSH host."""

import argparse
from datetime import datetime, timezone
import hashlib
from pathlib import Path
import re
import shlex
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("host", help="SSH config alias, or root@host with SSH configured")
    args = parser.parse_args()
    if args.host.startswith("-"):
        parser.error("invalid SSH host")
    root = Path(__file__).resolve().parents[3]
    source = root / "apps/desktop-gpui/kobo/ssh"
    artifacts = root / "target/desktop-gpui-kobo/persistent-ssh" / datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    artifacts.mkdir(parents=True)
    ssh = ["ssh", "-T", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=3", args.host]

    def remote(command):
        return subprocess.check_output(ssh + [command])

    # Inspect and save the live boot configuration before staging any changes.
    preflight = remote("""set -eu
[ "$(id -u)" = 0 ]
case "$(readlink /proc/1/exe)" in */busybox) ;; *) echo 'Expected BusyBox init' >&2; exit 1 ;; esac
test -f /etc/inittab && test ! -L /etc/inittab
test -f /mnt/onboard/.adds/bokheim/ssh/dropbear
readlink /proc/1/exe
df -Pk /etc /mnt/onboard
""")
    (artifacts / "preflight.log").write_bytes(preflight)
    (artifacts / "inittab.before-install").write_bytes(remote("cat /etc/inittab"))
    print(preflight.decode(), end="", flush=True)
    stage = remote("mktemp -d /mnt/onboard/.adds/bokheim-ssh-service.XXXXXX").decode().strip()
    if not re.fullmatch(r"/mnt/onboard/\.adds/bokheim-ssh-service\.[A-Za-z0-9]+", stage):
        raise RuntimeError("Invalid remote staging path")
    (artifacts / "remote-stage").write_text(stage + "\n")
    names = ["start.sh", "supervise.sh", "install-service.sh"]
    manifest = "".join(f"{hashlib.sha256((source / name).read_bytes()).hexdigest()}  {name}\n" for name in names)
    (artifacts / "checksums").write_text(manifest)
    try:
        for name in names + ["checksums"]:
            path = artifacts / name if name == "checksums" else source / name
            with path.open("rb") as payload:
                subprocess.run(ssh + [f"cat > {shlex.quote(stage + '/' + name)}"], stdin=payload, check=True)
        quoted_stage = shlex.quote(stage)
        install = f"""set -eu
cd {quoted_stage}
sha256sum -c checksums
for name in start.sh supervise.sh install-service.sh; do
    destination=/mnt/onboard/.adds/bokheim/ssh/$name
    temporary=$(mktemp /mnt/onboard/.adds/bokheim/ssh/service-script.XXXXXX)
    cp "$name" "$temporary"
    cmp "$name" "$temporary"
    mv -f "$temporary" "$destination"
done
/bin/sh /mnt/onboard/.adds/bokheim/ssh/install-service.sh
"""
        result = subprocess.run(ssh + [install], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        (artifacts / "install.log").write_bytes(result.stdout)
        print(result.stdout.decode(), end="", flush=True)
        result.check_returncode()
        # Observe the service wiring without rebooting or interrupting SSH.
        verify = """set -eu
grep -Fx '::respawn:/bin/sh /etc/bokheim-ssh/supervise.sh' /etc/inittab
attempt=0
while test "$attempt" -lt 20; do
    for proc in /proc/[0-9]*; do
        test -r "$proc/cmdline" || continue
        command=$(tr '\\000' ' ' < "$proc/cmdline")
        case "$command" in
            '/bin/sh /etc/bokheim-ssh/supervise.sh ')
                parent=$(awk '/^PPid:/ {print $2}' "$proc/status")
                if test "$parent" = 1; then
                    echo "SSH supervisor PID=${proc##*/}, parent=init"
                    ls -l /etc/bokheim-ssh/dropbear /etc/bokheim-ssh/supervise.sh /etc/bokheim-ssh/authorized_keys /etc/bokheim-ssh/hostkey
                    tail -n 10 /etc/bokheim-ssh/service.log
                    exit 0
                fi
                ;;
        esac
    done
    attempt=$((attempt+1))
    sleep 1
done
echo 'Boot entry installed, but supervisor was not observed' >&2
exit 1
"""
        observed = remote(verify)
        (artifacts / "verification.log").write_bytes(observed)
        print(observed.decode(), end="", flush=True)
        print(f"Persistent SSH installed; configuration and logs saved in {artifacts}")
    finally:
        files = " ".join(shlex.quote(stage + "/" + name) for name in names + ["checksums"])
        subprocess.run(ssh + [f"rm -f {files}; rmdir {shlex.quote(stage)}"], stdout=subprocess.DEVNULL, check=False)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(f"SSH service setup could not complete (command exit {error.returncode}).", file=sys.stderr)
        raise SystemExit(1) from None
