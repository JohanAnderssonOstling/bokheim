#!/usr/bin/env python3
"""Benchmark the open book on an SSH-connected Kobo using existing timing logs.

Usage: python3 scripts/benchmarks/kobo_page_turns.py --host bokheim-kobo
Chapter boundaries: add --scope chapter-boundary --first-key 194 --pairs 6

Keep the reader still with a book open. For ordinary page turns, start away
from chapter boundaries. Injects
Linux page-button events in alternating pairs, returning to the starting page.
Measures the installed binary, without rebuilding or restarting it. Measures
warm page turns or a repeated chapter boundary, not optical display latency.
For a chapter boundary, start on a chapter's first page and choose the key that
goes BACK first (Libra 2 landscape: 194). Verify the pages before running. The
scope label describes operator-selected positions; it cannot detect chapters.
Requires kobo_input_probe.c compiled with the existing ARM cross compiler using
-O2 -static and uploaded to /tmp/bokheim-input-probe (chmod 700), plus the
existing GPUI timing/scroll/hardware-button diagnostics. No tests
are compiled or run. Results and per-turn raw logs are saved together as JSON.
"""

import argparse
from datetime import datetime, timezone
import json
import math
from pathlib import Path
import re
import shlex
import statistics
import subprocess


LOG = "/mnt/onboard/.adds/bokheim/desktop-gpui-kobo.log"
BUILD = "/mnt/onboard/.adds/bokheim/BUILD-INFO.txt"


def summary(values):
    ordered = sorted(values)
    if not ordered:
        return None
    return {
        "count": len(ordered),
        "median_ms": statistics.median(ordered),
        "p95_ms": ordered[math.ceil(len(ordered) * 0.95) - 1],
        "min_ms": ordered[0],
        "max_ms": ordered[-1],
    }


def parse_turn(log):
    timestamped = []
    for line in log.splitlines():
        timestamp, content = line.split(" ", 1)
        timestamped.append((float(timestamp), content))
    log = "\n".join(content for _, content in timestamped)
    turns = re.findall(r"hardware button complete: (NextPage|PreviousPage) total_ms=(\d+)", log)
    scrolls = re.findall(r"GPUI scroll timing: pages=([\d.-]+) dispatch_ms=(\d+) frame_ms=(\d+) total_ms=(\d+)", log)
    frames = [{"observed_ms": timestamp, **dict(re.findall(r"(\w+)=(\S+)", line))} for timestamp, line in timestamped if line.startswith("GPUI timing:")]
    if len(turns) != 1 or len(scrolls) != 1:
        raise RuntimeError("Expected one page-button completion and scroll; reader may be asleep or receiving other input.")
    if "rotation" in log or "KOBO_EXPERIENCE" in log:
        raise RuntimeError("Screen orientation or reader mode changed during the benchmark.")
    changed = [frame for frame in frames if frame.get("changed") == "true"]
    synchronous = [frame for frame in changed if frame.get("scrolling") == "true"]
    if not changed:
        raise RuntimeError("Page button produced no changed frame; open a book away from its first/last page.")
    return {
        "button": turns[0][0],
        "button_handler_ms": int(turns[0][1]),
        "dispatch_ms": int(scrolls[0][1]),
        "synchronous": len(synchronous) == 1 and len(changed) == 1,
        "changed_frames": changed,
        "input_to_first_presentation_ms": changed[0]["observed_ms"],
        "input_to_final_presentation_ms": changed[-1]["observed_ms"],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--host", default="bokheim-kobo")
    parser.add_argument("--scope", choices=("pages", "chapter-boundary"), default="pages")
    parser.add_argument("--first-key", type=int, choices=(193, 194), default=194)
    parser.add_argument("--probe", default="/tmp/bokheim-input-probe")
    parser.add_argument("--pairs", type=int, default=20)
    parser.add_argument("--warmup-pairs", type=int, default=2)
    parser.add_argument("--settle", type=float, default=4.0, help="Device-side observation window in seconds per turn")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.host.startswith("-") or not 1 <= args.pairs <= 1000 or not 0 <= args.warmup_pairs <= 100 or not math.isfinite(args.settle) or not 2 <= args.settle <= 120:
        parser.error("Invalid host, pair count or observation window (2–120 seconds)")

    def ssh(command, data=None):
        return subprocess.run(
            ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=6", args.host, command],
            input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True, timeout=max(20, args.settle + 10),
        ).stdout

    log_path = shlex.quote(LOG)
    initial_log = ssh(f"cat {log_path}").decode()
    device = re.findall(r"^KOBO_DEVICE .*", initial_log, re.MULTILINE)
    modes = re.findall(r"^KOBO_EXPERIENCE .*", initial_log, re.MULTILINE)
    if not modes or not re.search(r"(?:initial=|->)ReaderQuality\b", modes[-1]):
        raise RuntimeError("Open a book first; latest logged experience mode is not ReaderQuality.")
    metadata = ssh(
        f"uname -m; cat {shlex.quote(BUILD)}; "
        "printf '\\nCPU governor/frequency:\\n'; "
        "cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor "
        "/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq; "
        "printf '\\nInput devices:\\n'; cat /proc/bus/input/devices"
    ).decode()
    if not metadata.startswith("armv7l\n"):
        raise RuntimeError("This event format requires a 32-bit ARM Kobo.")
    started = datetime.now(timezone.utc)
    output = args.output or Path(__file__).with_name("results") / f"kobo-page-turns-{started.strftime('%Y%m%dT%H%M%SZ')}.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    result = {
        "started_utc": started.isoformat(), "host": args.host,
        "device": device[-1] if device else None, "experience": modes[-1],
        "metadata": metadata, "probe": args.probe, "scope": args.scope,
        "pairs": args.pairs, "warmup_pairs": args.warmup_pairs, "settle_seconds": args.settle,
        "conditions": "Repeated adjacent pages or selected chapter boundary; installed build, warm caches, existing diagnostics enabled.",
        "limitations": [
            "Handler timings exclude hardware/input-poll latency and do not measure optical screen settling.",
            "Partial non-flashing refresh presentation may return before the panel finishes updating.",
            "Input-to-presentation observes the final changed-frame log, including asynchronous frames; approximately 2 ms polling resolution, plus scheduling overhead.",
            "Observation window must include all relevant frames; multiple changed frames are retained for inspection.",
            "Warm repeated boundary measurements are not cold-cache loading or a survey of all chapters.",
            "Alternating inputs intend to restore position; interruptions or page boundaries can prevent restoration.",
        ],
        "samples": [],
    }
    print(f"Benchmarking {result['device']}: {args.pairs * 2} measured turns", flush=True)
    try:
        for pair in range(args.warmup_pairs + args.pairs):
            for code in (args.first_key, 387 - args.first_key):
                log = ssh(f"{shlex.quote(args.probe)} key {code} {args.settle}").decode(errors="replace")
                sample = {"pair": pair, "warmup": pair < args.warmup_pairs, "key_code": code, "log": log}
                result["samples"].append(sample)
                sample.update(parse_turn(log))
                print(f"{'warmup' if sample['warmup'] else 'sample'} {pair + 1}: {sample['button']} presentation={sample['input_to_final_presentation_ms']:.1f} ms, handler={sample['button_handler_ms']} ms", flush=True)
    except (RuntimeError, subprocess.SubprocessError, KeyboardInterrupt) as error:
        result["error"] = str(error) or type(error).__name__
        raise
    finally:
        measured = [s for s in result["samples"] if not s["warmup"] and "synchronous" in s]
        synchronous = [s for s in measured if s["synchronous"]]
        result["summary"] = {
            "measured_turns": len(measured), "synchronous_turns": len(synchronous),
            "asynchronous_or_multiframe_turns": len(measured) - len(synchronous),
            "input_to_final_presentation": summary([s["input_to_final_presentation_ms"] for s in measured]),
            "synchronous_button_handler": summary([s["button_handler_ms"] for s in synchronous]),
            **{name: summary([int(s["changed_frames"][-1][field]) for s in measured]) for name, field in (
                ("render", "render_ms"), ("presentation_call", "present_ms"),
            )},
            "dispatch": summary([s["dispatch_ms"] for s in measured]),
            "by_button": {button: summary([s["input_to_final_presentation_ms"] for s in measured if s["button"] == button]) for button in ("NextPage", "PreviousPage")},
        }
        output.write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps(result["summary"], indent=2))
        print(f"Saved {output}")


if __name__ == "__main__":
    main()
