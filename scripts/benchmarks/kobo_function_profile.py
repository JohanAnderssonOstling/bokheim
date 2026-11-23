#!/usr/bin/env python3
"""Sample exclusive userspace CPU functions during repeated Kobo chapter turns.

Requires kobo_cpu_sample.c and kobo_input_probe.c built with the existing ARM
compiler (-O2 -static), uploaded to /tmp/bokheim-cpu-sample and
/tmp/bokheim-input-probe, and chmod 700. No application build or restart.
Start on a chapter's first page. This captures PreviousPage then NextPage
(Libra 2 landscape keys 194,193). Verify the boundary beforehand.
Function symbols come from the actual installed ELF files, not a local build.
CPU samples are bounded by each injected input and final presentation log;
they are exclusive instruction-pointer samples, not inclusive function timers.
"""

import argparse
from bisect import bisect_right
from collections import Counter
from datetime import datetime, timezone
import gzip
import json
from pathlib import Path
import re
import shlex
import struct
import subprocess
import time

from kobo_page_turns import parse_turn, summary


class ElfSymbols:
    def __init__(self, ssh, path):
        self.ssh, self.path = ssh, path
        header = ssh(f"head -c 52 {shlex.quote(path)}")
        if header[:6] != b"\x7fELF\x01\x01":
            raise RuntimeError(f"Expected a little-endian ELF32 file: {path}")
        section_offset = struct.unpack_from("<I", header, 32)[0]
        section_size, section_count = struct.unpack_from("<HH", header, 46)
        raw = self.read(section_offset, section_size * section_count)
        sections = [struct.unpack_from("<IIIIIIIIII", raw, i * section_size) for i in range(section_count)]
        symbols = {}
        # Prefer the full symbol table; shared libraries may only have dynsym.
        tables = [s for s in sections if s[1] == 2] or [s for s in sections if s[1] == 11]
        self.symbol_bytes = sum(s[5] + sections[s[6]][5] for s in tables)
        for section in tables:
            strings_section = sections[section[6]]
            strings = self.read(strings_section[4], strings_section[5])
            entries = self.read(section[4], section[5])
            for offset in range(0, len(entries), section[9]):
                name, address, size, info, _, index = struct.unpack_from("<IIIBBH", entries, offset)
                if index == 0 or info & 15 not in (2, 10) or not size:
                    continue
                end = strings.find(b"\x00", name)
                symbol = strings[name:end].decode(errors="replace")
                address &= ~1  # ARM Thumb function pointers carry a mode bit.
                if address not in symbols or size > symbols[address][0]:
                    symbols[address] = (size, symbol)
        self.addresses = sorted(symbols)
        self.symbols = symbols

    def read(self, offset, size):
        if not size:
            return b""
        block = 65536
        start, within = divmod(offset, block)
        count = (within + size + block - 1) // block
        compressed = self.ssh(f"dd if={shlex.quote(self.path)} bs={block} skip={start} count={count} 2>/dev/null | gzip -1")
        data = gzip.decompress(compressed)[within:within + size]
        if len(data) != size:
            raise RuntimeError(f"Short ELF read for {self.path}")
        return data

    def resolve(self, address):
        index = bisect_right(self.addresses, address) - 1
        if index >= 0:
            start = self.addresses[index]
            size, name = self.symbols[start]
            if address < start + size:
                return name
        return f"[unknown+0x{address:x}]"


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--host", default="bokheim-kobo")
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--analyze-only", action="store_true", help="Re-symbolize the saved capture without device input")
    parser.add_argument("--pairs", type=int, default=3)
    parser.add_argument("--callers", action="store_true", help="Capture user callchains, ARM registers and a bounded stack snapshot")
    parser.add_argument("--first-key", type=int, choices=(193, 194), default=194)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.host.startswith("-") or args.pid <= 0 or not 1 <= args.pairs <= 20:
        parser.error("Invalid host, PID or pair count (1–20)")

    ssh_base = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=6", args.host]

    def ssh(command):
        return subprocess.check_output(ssh_base + [command], timeout=90)

    if args.analyze_only:
        result = json.loads(args.output.read_text())
        analyze(args, ssh, result, args.output.with_suffix(".samples.txt"), result["maps"])
        return

    args.output.parent.mkdir(parents=True, exist_ok=True)
    raw_path = args.output.with_suffix(".samples.txt")
    maps_text = ssh(f"cat /proc/{args.pid}/maps").decode()
    threads_text = ssh(f"for p in /proc/{args.pid}/task/*/comm; do printf '%s ' \"$p\"; cat \"$p\"; done").decode()
    result = {
        "captured_utc": datetime.now(timezone.utc).isoformat(), "pid": args.pid,
        "host": args.host, "method": "perf_event_open software CPU clock, exclusive userspace instruction-pointer sampling, 500 Hz per active thread",
        "scope": "Repeated verified chapter ONE end <-> chapter TWO first page; warm installed build",
        "limitations": [
            "Statistical exclusive CPU attribution; inlined callees may be charged to their containing symbol.",
            "CPU totals across threads can overlap in wall time. Waiting and kernel execution are excluded.",
            "Symbols are fetched from the exact installed ELF files after capture.",
            "A new thread created after sampler attachment is not sampled.",
        ],
        "maps": maps_text, "threads": threads_text, "turns": [], "raw_samples": raw_path.name,
        "caller_snapshots": args.callers,
    }
    duration = args.pairs * 8 + 10
    with raw_path.open("wb") as raw:
        caller_option = " --callers" if args.callers else ""
        sampler = subprocess.Popen(ssh_base + [f"/tmp/bokheim-cpu-sample {args.pid} {duration}{caller_option}"], stdout=raw, stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                with raw_path.open() as reader:
                    ready = reader.readline().strip()
                if ready.startswith("READY "):
                    result["sampler"] = ready
                    print(ready, flush=True)
                    break
                if sampler.poll() is not None:
                    raise RuntimeError(sampler.stderr.read().decode())
                time.sleep(0.05)
            else:
                raise RuntimeError("Sampler did not become ready")
            for pair in range(args.pairs):
                for code in (args.first_key, 387 - args.first_key):
                    log = ssh(f"/tmp/bokheim-input-probe key {code} 3").decode()
                    turn = {"pair": pair, "log": log, **parse_turn(log)}
                    if turn["synchronous"]:
                        raise RuntimeError("Expected asynchronous chapter loading; input reached an ordinary page or sidebar. Verify the chapter boundary.")
                    start = int(re.search(r"PROBE_START monotonic_ns=(\d+)", log)[1])
                    turn["start_ns"] = start
                    turn["end_ns"] = start + int(turn["input_to_final_presentation_ms"] * 1e6)
                    result["turns"].append(turn)
                    print(f"pair {pair + 1} {turn['button']} {turn['input_to_final_presentation_ms']:.1f} ms", flush=True)
            stderr = sampler.communicate(timeout=duration + 10)[1]
            if sampler.returncode:
                raise RuntimeError(stderr.decode())
        except BaseException:
            sampler.terminate()
            sampler.communicate(timeout=10)
            args.output.write_text(json.dumps(result, indent=2) + "\n")
            raise

    analyze(args, ssh, result, raw_path, maps_text)


def analyze(args, ssh, result, raw_path, maps_text):
    executable_maps = []
    for line in maps_text.splitlines():
        parts = line.split(None, 5)
        if "x" in parts[1] and len(parts) == 6 and parts[5].startswith("/"):
            low, high = (int(value, 16) for value in parts[0].split("-"))
            executable_maps.append((low, high, int(parts[2], 16), parts[5]))
    samples = []
    lost = 0
    for line in raw_path.read_text().splitlines():
        fields = line.split()
        if fields[0] == "L":
            lost += int(fields[1])
        if fields[0] != "S":
            continue
        timestamp, tid, ip, period = int(fields[1]), int(fields[2]), int(fields[3], 16) & ~1, int(fields[4])
        turn = next((turn for turn in result["turns"] if turn["start_ns"] <= timestamp <= turn["end_ns"]), None)
        if turn is not None:
            samples.append((turn["button"], tid, ip, period))
    tables = {}
    counts = Counter()
    cpu_ns = Counter()
    by_button = {button: Counter() for button in ("NextPage", "PreviousPage")}
    by_button_counts = {button: Counter() for button in by_button}
    by_tid = Counter()
    thread_functions = {}
    thread_counts = {}
    sampled_symbols = {}
    for button, tid, ip, period in samples:
        mapping = next((m for m in executable_maps if m[0] <= ip < m[1]), None)
        if mapping:
            low, _, offset, path = mapping
            if path not in tables:
                print(f"Reading exact installed symbols: {path}", flush=True)
                tables[path] = ElfSymbols(ssh, path)
            name = tables[path].resolve(ip - low + offset)
        else:
            path, name = "[unmapped]", f"0x{ip:x}"
        key = (path, name)
        counts[key] += 1
        cpu_ns[key] += period
        by_button[button][key] += period
        by_button_counts[button][key] += 1
        by_tid[tid] += period
        thread_functions.setdefault(tid, Counter())[key] += period
        thread_counts.setdefault(tid, Counter())[key] += 1
        sampled_symbols[str(ip)] = {"module": path, "mangled": name}
    names = sorted({name for _, name in counts})
    demangled = subprocess.check_output(["c++filt", "-s", "rust"], input="\n".join(names) + "\n", text=True).splitlines() if names else []
    translations = dict(zip(names, demangled))

    def rows(values, total, sample_counts=None):
        sample_counts = counts if sample_counts is None else sample_counts
        return [{
            "function": translations.get(name, name), "mangled": name, "module": path,
            "samples": sample_counts[(path, name)], "cpu_ms_estimate": value / 1e6,
            "cpu_percent": value / total * 100 if total else 0,
        } for (path, name), value in values.most_common()]

    total = sum(cpu_ns.values())
    result.update({
        "sample_count": len(samples), "lost_samples": lost,
        "cpu_ms_estimate": total / 1e6, "functions": rows(cpu_ns, total),
        "by_button": {button: {"cpu_ms_estimate": sum(values.values()) / 1e6, "functions": rows(values, sum(values.values()), by_button_counts[button])} for button, values in by_button.items()},
        "by_tid_cpu_ms_estimate": {str(tid): value / 1e6 for tid, value in by_tid.items()},
        "by_thread": {str(tid): {"cpu_ms_estimate": sum(values.values()) / 1e6, "functions": rows(values, sum(values.values()), thread_counts[tid])} for tid, values in thread_functions.items()},
        "sampled_symbols": sampled_symbols,
        "wall_time_by_button": {button: summary([turn["input_to_final_presentation_ms"] for turn in result["turns"] if turn["button"] == button]) for button in by_button},
        "symbol_table_bytes": {path: table.symbol_bytes for path, table in tables.items()},
    })
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"sample_count": len(samples), "lost": lost, "top_functions": result["functions"][:20]}, indent=2))
    print(f"Saved {args.output}")


if __name__ == "__main__":
    main()
