#!/usr/bin/env python3
"""Benchmark release startup using sanitized SQLite backups of local app data.

Build with: cargo build --release -p desktop-gpui --example startup-profile
Then run: python3 apps/desktop-gpui/scripts/benchmark-startup.py \
    --source-app-data ~/.local/share/bokheim

Runs fresh processes with warm filesystem caches. Source databases are opened
read-only and backed up through SQLite, including committed WAL contents. Each
trial gets fresh copies, signed-out account state, empty isolated content roots,
and a local catalog. Results exclude Android-specific startup and model copying.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import statistics
import subprocess
import tempfile
import time


TIMING = re.compile(
    r"scope=(\S+) stage=(\S+) duration_ms=([\d.]+) "
    r"elapsed_ms=([\d.]+) since_start_ms=([\d.]+)"
)


def backup(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(source.resolve().as_uri() + "?mode=ro", uri=True) as reader:
        with sqlite3.connect(destination) as writer:
            reader.backup(writer)


def snapshot(source, destination):
    backup(source / "app-state-v1.sqlite3", destination / "app-state-v1.sqlite3")
    with sqlite3.connect(destination / "app-state-v1.sqlite3") as db:
        libraries = [row[0] for row in db.execute("SELECT library_id FROM library_registry ORDER BY library_id")]
        db.execute("DELETE FROM app_value WHERE key NOT IN ('browsing_preferences.json', 'reader_preferences.json')")
        db.execute("DELETE FROM detached_library")
        db.execute("DELETE FROM pending_local_purge")
    if not libraries:
        raise RuntimeError("Source app data must contain at least one library")
    for library in libraries:
        backup(source / "libraries" / library / "library.db", destination / "libraries" / library / "library.db")
    backup(source / "catalog.sqlite3", destination / "catalog.sqlite3")
    session = json.loads((source / "gpui-ui-session.json").read_text())
    # Keep the saved browse destination, but do not open a reader or a detail.
    session["navigation"]["reader"] = None
    for page in session["navigation"].get("pages", []):
        page["detail"] = None
    if session["navigation"].get("library_id") not in libraries:
        session["navigation"] = {"library_id": libraries[0], "route": {"Library": "Catalog"}, "pages": []}
    (destination / "gpui-ui-session.json").write_text(json.dumps(session))
    return libraries, session


def prepare_trial(seed, destination, selected, session):
    backup(seed / "app-state-v1.sqlite3", destination / "app-state-v1.sqlite3")
    backup(seed / "catalog.sqlite3", destination / "catalog.sqlite3")
    placeholders = ",".join("?" for _ in selected)
    with sqlite3.connect(destination / "app-state-v1.sqlite3") as db:
        db.execute(f"DELETE FROM library_registry WHERE library_id NOT IN ({placeholders})", selected)
        for library in selected:
            content = destination / "content" / library
            content.mkdir(parents=True)
            db.execute("UPDATE library_registry SET storage_path=? WHERE library_id=?", (str(content), library))
    for library in selected:
        backup(seed / "libraries" / library / "library.db", destination / "libraries" / library / "library.db")
    (destination / "gpui-ui-session.json").write_text(json.dumps(session))


def metrics(log):
    values = {}
    library_times = []
    for match in TIMING.finditer(log):
        scope, stage = match[1], match[2]
        duration, elapsed, since_start = map(float, match.group(3, 4, 5))
        if scope == "library_open":
            library_times.append(duration)
        elif scope in ("browse", "launch"):
            values.setdefault(f"{scope}.{stage}.since_start_ms", since_start)
            if scope == "launch":
                values.setdefault(f"{scope}.{stage}.duration_ms", duration)
        else:
            values.setdefault(f"{scope}.{stage}.duration_ms", duration)
    values["library_open.sum_ms"] = sum(library_times)
    values["library_open.count"] = len(library_times)
    return values


def main():
    project = Path(__file__).resolve().parents[3]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-app-data", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=project / "target/startup-benchmark/results")
    parser.add_argument("--binary", type=Path, default=project / "target/release/examples/startup-profile")
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    records = []
    def launch(directory, name, deferred=False):
        environment = os.environ.copy()
        temporary_files = directory / "tmp"
        temporary_files.mkdir(exist_ok=True)
        environment["TMPDIR"] = str(temporary_files)
        environment["RUST_LOG"] = "warn,startup_profile=debug,browse_profile=debug"
        # The developer catalog override deliberately forces a projection
        # rebuild, which would misrepresent an ordinary repeat launch.
        environment.pop("BOKHEIM_CATALOG_FILE", None)
        environment.pop("BOKHEIM_STARTUP_DEFER_MAINTENANCE", None)
        if deferred:
            environment["BOKHEIM_STARTUP_DEFER_MAINTENANCE"] = "1"
        log_path = output / f"{name}.log"
        with log_path.open("w") as stream:
            process = subprocess.Popen([str(args.binary.resolve()), str(directory)], env=environment,
                                       stdout=stream, stderr=subprocess.STDOUT, cwd=project,
                                       start_new_session=True)
            try:
                returncode = process.wait(timeout=90)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
                raise RuntimeError(f"Startup timed out; inspect {log_path}")
        values = metrics(log_path.read_text())
        if returncode or "launch.frame_callback.since_start_ms" not in values:
            raise RuntimeError(f"Startup failed ({returncode}); inspect {log_path}")
        if "browse.listing_frame_callback.since_start_ms" not in values:
            raise RuntimeError(f"No rendered browse listing; inspect {log_path}")
        return values, log_path

    with tempfile.TemporaryDirectory(prefix="startup-data-", dir=output) as temporary:
        root = Path(temporary)
        snapshot_directory = root / "snapshot"
        seed = root / "seed"
        libraries, session = snapshot(args.source_app_data, snapshot_directory)
        active = session["navigation"]["library_id"]
        libraries = [active] + [library for library in libraries if library != active]
        prepare_trial(snapshot_directory, seed, libraries, session)
        print("Preparing a current-schema seed; first-open costs are recorded separately...", flush=True)
        first_open, _ = launch(seed, "seed-first-open", deferred=True)
        (output / "seed-first-open.json").write_text(json.dumps(first_open, indent=2) + "\n")
        counts = sorted({1, len(libraries)})
        for iteration in range(args.runs):
            for count in counts:
                for deferred in (False, True):
                    case = f"libraries-{count}-{'deferred-to-frame' if deferred else 'normal'}"
                    name = f"{case}-{iteration + 1:02}"
                    trial = root / name
                    prepare_trial(seed, trial, libraries[:count], session)
                    started = time.monotonic()
                    values, log_path = launch(trial, name, deferred)
                    record = {"case": case, "iteration": iteration + 1, "process_seconds": time.monotonic() - started,
                              "metrics": values, "log": log_path.name}
                    records.append(record)
                    (output / "runs.json").write_text(json.dumps(records, indent=2) + "\n")
                    print(f"{name}: fonts={values['open_ui.fonts_register.duration_ms']:.2f} ms, "
                          f"backend={values['open_ui.backend_start.duration_ms']:.2f} ms, "
                          f"frame={values['launch.frame_callback.since_start_ms']:.2f} ms, "
                          f"listing={values['browse.listing_frame_callback.since_start_ms']:.2f} ms", flush=True)
                    shutil.rmtree(trial)
        summary = {}
        for case in sorted({record["case"] for record in records}):
            samples = [record["metrics"] for record in records if record["case"] == case]
            summary[case] = {
                key: {"median": statistics.median(sample[key] for sample in samples),
                      "min": min(sample[key] for sample in samples), "max": max(sample[key] for sample in samples)}
                for key in samples[0] if all(key in sample for sample in samples)
            }
        (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(f"Results: {output / 'summary.json'}")


if __name__ == "__main__":
    main()
