#!/usr/bin/env python3
"""Measure process-cold launches on an unlocked Android device.

Requires an optimized APK containing the native startup-profile instrumentation.
App data is preserved. The first launch is recorded separately; each measured
launch force-stops the app, then enables profiling through an intent extra.
Filesystem caches remain warm. GPUI callbacks do not measure panel presentation.
"""
import argparse
import atexit
import json
from pathlib import Path
import re
import statistics
import subprocess
import time


TIMING = re.compile(r"scope=(\S+) stage=(\S+) duration_ms=([\d.]+) elapsed_ms=([\d.]+) since_start_ms=([\d.]+)")
LOG_LINE = re.compile(r"^\s*(\d+\.\d+)\s+(\d+)\s+\d+\s+[VDIWEF]\s+[^:]+:\s+(.*)$")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--defer-background", action="store_true", help="hold backend asynchronous tasks until the first GPUI frame")
    parser.add_argument("--no-defer-maintenance", action="store_true", help="start library maintenance immediately for comparison")
    parser.add_argument("--android-system-fonts", action="store_true", help="load Android system fonts in addition to Bokheim's bundled fonts")
    parser.add_argument("--no-gpu-cache", action="store_true", help="disable persistent GPU pipeline caching for comparison")
    parser.add_argument("--force-opengl", action="store_true", help="exercise the OpenGL rendering backend")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parents[3] / "target/startup-benchmark/android/results")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    adb = ["adb", "-s", args.serial]
    package = "se.bokheim.reader.gpui"
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)

    def shell(*command, timeout=30):
        return subprocess.check_output(adb + ["shell", *command], text=True, timeout=timeout)

    window_state = shell("dumpsys", "window")
    if "mDreamingLockscreen=true" in window_state or "mShowingLockscreen=true" in window_state:
        raise RuntimeError("Unlock the phone before measuring startup")
    previous_stay_awake = shell("settings", "get", "global", "stay_on_while_plugged_in").strip()
    def restore_screen_setting():
        if previous_stay_awake == "null":
            shell("settings", "delete", "global", "stay_on_while_plugged_in")
        else:
            shell("settings", "put", "global", "stay_on_while_plugged_in", previous_stay_awake)
    atexit.register(restore_screen_setting)
    shell("svc", "power", "stayon", "true")
    environment = {
        "serial": args.serial, "model": shell("getprop", "ro.product.model").strip(),
        "android": shell("getprop", "ro.build.version.release").strip(),
        "soc": shell("getprop", "ro.soc.model").strip(),
        "package": package, "runs": args.runs, "battery_before": shell("dumpsys", "battery"),
        "defer_background_until_frame": args.defer_background,
        "defer_library_maintenance_until_frame": not args.no_defer_maintenance,
        "android_system_fonts": args.android_system_fonts,
        "gpu_pipeline_cache": not args.no_gpu_cache,
        "force_opengl": args.force_opengl,
        "conditions": ["release native build", "existing app data and saved navigation", "force-stop before each launch",
                       "warm filesystem caches", "first launch excluded from medians", "GPUI frame callbacks, not panel presentation"],
    }
    (output / "environment.json").write_text(json.dumps(environment, indent=2) + "\n")
    records = []
    for index in range(args.runs + 1):
        name = "first-launch" if index == 0 else f"run-{index:02}"
        shell("am", "force-stop", package)
        time.sleep(1)
        clock_before = time.time()
        device_time = float(shell("date", "+%s.%N").strip())
        clock_after = time.time()
        clock_offset = device_time - (clock_before + clock_after) / 2
        log_path = output / f"{name}.log"
        with log_path.open("w") as stream:
            logger = subprocess.Popen(adb + ["logcat", "-v", "epoch", "-T", "1", "gpui:I", "ActivityTaskManager:I", "AndroidRuntime:E", "*:S"],
                                      stdout=stream, stderr=subprocess.STDOUT)
            try:
                launched_at = time.time()
                launch = shell("am", "start", "-W", "-n", f"{package}/.MainActivity", "--ez", "bokheim.startup_profile", "true",
                               "--ez", "bokheim.startup_defer_background", "true" if args.defer_background else "false",
                               "--ez", "bokheim.startup_defer_maintenance", "false" if args.no_defer_maintenance else "true",
                               "--ez", "bokheim.android_system_fonts", "true" if args.android_system_fonts else "false",
                               "--ez", "bokheim.gpu_pipeline_cache", "false" if args.no_gpu_cache else "true",
                               "--ez", "bokheim.force_opengl", "true" if args.force_opengl else "false", timeout=90)
                (output / f"{name}.am-start.txt").write_text(launch)
                pid = shell("pidof", package).strip().split()[0]
                deadline = time.monotonic() + 90
                ready_since = None
                while time.monotonic() < deadline:
                    text = log_path.read_text(errors="replace")
                    lines = [match for line in text.splitlines() if (match := LOG_LINE.match(line)) and match[2] == pid and float(match[1]) - clock_offset >= launched_at - 0.5]
                    messages = "\n".join(match[3] for match in lines)
                    if "stage=frame_callback" in messages:
                        ready_since = ready_since or time.monotonic()
                        # Allow async content, cover preparation and telemetry
                        # to finish before stopping this measured process.
                        if time.monotonic() - ready_since >= 5:
                            break
                    time.sleep(0.25)
                else:
                    raise RuntimeError(f"No first-frame callback; inspect {log_path}")
            finally:
                logger.terminate()
                try:
                    logger.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    logger.kill()
                    logger.wait()
        metrics = {}
        per_library = []
        profile_events = []
        thread_libraries = {}
        for line in log_path.read_text(errors="replace").splitlines():
            parsed = LOG_LINE.match(line)
            if not parsed or parsed[2] != pid or float(parsed[1]) - clock_offset < launched_at - 0.5:
                continue
            match = TIMING.search(parsed[3])
            if not match:
                continue
            scope, stage = match[1], match[2]
            duration, elapsed, since_start = map(float, match.group(3, 4, 5))
            thread_id = line.split(None, 3)[2]
            library = re.search(r"\blibrary_id=(\S+)", parsed[3])
            if library and library[1] != "-":
                thread_libraries[thread_id] = library[1]
            profile_events.append({"scope": scope, "stage": stage, "duration_ms": duration,
                                   "elapsed_ms": elapsed, "since_start_ms": since_start,
                                   "library_id": thread_libraries.get(thread_id), "thread_id": thread_id})
            if scope == "library_open":
                per_library.append(duration)
            else:
                metrics.setdefault(f"{scope}.{stage}.duration_ms", duration)
                metrics.setdefault(f"{scope}.{stage}.since_start_ms", since_start)
                metrics.setdefault(f"{scope}.{stage}.launch_to_log_ms", (float(parsed[1]) - clock_offset - launched_at) * 1000)
        metrics["library_open.count"] = len(per_library)
        metrics["library_open.sum_ms"] = sum(per_library)
        library_end = metrics.get("backend_construct.libraries_open.since_start_ms", float("inf"))
        breakdown = {}
        for event in profile_events:
            if not event["scope"].startswith("library_") or not event["library_id"] or event["since_start_ms"] > library_end:
                continue
            key = f"{event['scope']}.{event['stage']}.sum_ms"
            metrics[key] = metrics.get(key, 0) + event["duration_ms"]
            if event["library_id"]:
                stages = breakdown.setdefault(event["library_id"], {})
                stages[key] = stages.get(key, 0) + event["duration_ms"]
        if "android_launch.frame_callback.since_start_ms" not in metrics:
            raise RuntimeError(f"Startup profile missing; inspect {log_path}")
        for field in ("TotalTime", "WaitTime", "ThisTime"):
            if match := re.search(rf"^{field}: (\d+)$", launch, re.M):
                metrics[f"android.{field}.ms"] = int(match[1])
        record = {"name": name, "pid": pid, "metrics": metrics, "per_library_ms": per_library, "log": log_path.name,
                  "device_clock_offset_seconds": clock_offset, "clock_uncertainty_ms": (clock_after - clock_before) * 500,
                  "profile_events": profile_events, "library_breakdown": breakdown}
        records.append(record)
        (output / "runs.json").write_text(json.dumps(records, indent=2) + "\n")
        listing = metrics.get("browse.listing_frame_callback.since_start_ms")
        print(f"{name}: fonts={metrics['open_ui.fonts_register.duration_ms']:.2f} ms, "
              f"backend={metrics['open_ui.backend_start.duration_ms']:.2f} ms, "
              f"native-to-frame={metrics['android_launch.frame_callback.since_start_ms']:.2f} ms, "
              f"native-to-listing={listing}, libraries={len(per_library)}", flush=True)
    measured = [record["metrics"] for record in records[1:]]
    summary = {
        key: {"median": statistics.median(sample[key] for sample in measured),
              "min": min(sample[key] for sample in measured), "max": max(sample[key] for sample in measured)}
        for key in measured[0] if all(key in sample for sample in measured)
    }
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    environment["battery_after"] = shell("dumpsys", "battery")
    (output / "environment.json").write_text(json.dumps(environment, indent=2) + "\n")
    print(f"Results: {output / 'summary.json'}")


if __name__ == "__main__":
    main()
