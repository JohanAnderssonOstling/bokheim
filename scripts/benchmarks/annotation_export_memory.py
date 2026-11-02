#!/usr/bin/env python3
"""Measure production annotation exports in fresh processes on synthetic libraries.

Build first:
    cargo build --release -p library-annotation-export --example export_memory
Run:
    python3 scripts/benchmarks/annotation_export_memory.py --books 100 1000
Results and generated libraries are saved in a new local temporary directory.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile


def invoke(arguments):
    process = subprocess.run(arguments, text=True, capture_output=True)
    if process.returncode:
        raise RuntimeError(f"Command failed: {arguments}\n{process.stdout}\n{process.stderr}")
    return process.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/examples/export_memory"))
    parser.add_argument("--books", type=int, nargs="+", default=[100, 1000])
    parser.add_argument("--annotations", type=int, default=25)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--modes", nargs="+", choices=["first", "replace", "markdown", "single"], default=["first", "replace", "markdown", "single"])
    parser.add_argument("--work-parent", type=Path, default=Path.home() / ".cache")
    args = parser.parse_args()
    if min(*args.books, args.annotations, args.repeats) < 1:
        parser.error("sizes and repeats must be positive")
    binary = args.binary.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="bokheim-annotation-memory-", dir=args.work_parent))
    with binary.open("rb") as executable:
        binary_sha256 = hashlib.file_digest(executable, "sha256").hexdigest()
    result = {
        "started_utc": datetime.now(timezone.utc).isoformat(), "binary_sha256": binary_sha256,
        "platform": platform.platform(), "cpu_count": os.cpu_count(),
        "cpu_model": next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")), "unknown"),
        "binary": str(binary), "work_directory": str(root),
        "notes": "Release production export API; fresh process per sample; Linux VmHWM; synthetic EPUBs, empty TOC; excludes app/UI and setup; single uses scoped export API, excludes scheduling/receipts.",
        "samples": [], "summaries": [],
    }
    print(f"Results: {root / 'results.json'}", flush=True)
    for books in args.books:
        fixture = root / f"books-{books}-annotations-{args.annotations}"
        setup = invoke([str(binary), "setup", str(fixture), str(books), str(args.annotations)])
        print(f"Prepared: {setup.strip()}", flush=True)
        for mode in args.modes:
            samples = []
            for repeat in range(args.repeats):
                output = invoke([str(binary), "run", str(fixture), mode])
                sample = json.loads(output)
                sample["repeat"] = repeat + 1
                samples.append(sample)
                result["samples"].append(sample)
                (root / "results.json").write_text(json.dumps(result, indent=2) + "\n")
                print(json.dumps(sample), flush=True)
            summary = {"books": books, "annotations_per_book": args.annotations, "mode": mode}
            for key in ("before_kib", "after_preparation_kib", "peak_kib", "end_kib", "elapsed_ms", "existing_json_bytes"):
                summary[key] = statistics.median(sample[key] for sample in samples)
            result["summaries"].append(summary)
            (root / "results.json").write_text(json.dumps(result, indent=2) + "\n")
    print("\nBooks | Mode | Peak RSS MiB | Before RSS MiB | Seconds (median)")
    for item in result["summaries"]:
        print(f"{item['books']} | {item['mode']} | {item['peak_kib']/1024:.1f} | {item['before_kib']/1024:.1f} | {item['elapsed_ms']/1000:.3f}")


if __name__ == "__main__":
    main()
