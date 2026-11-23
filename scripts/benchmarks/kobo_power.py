#!/usr/bin/env python3
"""Summarize or compare TSV captures made by kobo_power.sh on the Kobo.

Usage: python3 scripts/benchmarks/kobo_power.py bokheim-idle.tsv nickel-idle.tsv
Reports whole-device battery consumption, not power attributable to one process.
Gauge values use the standard Linux micro-unit ABI, except that the recorder
normalizes the Ricoh619 vendor driver's mislabeled remaining-charge reading.
Inspect the actual device's readings before relying on their units or resolution.
"""

import argparse
import csv
import json
import math
from pathlib import Path
import statistics


def number(row, field):
    try:
        value = float(row[field])
        return value if math.isfinite(value) else None
    except (KeyError, TypeError, ValueError):
        return None


def summarize(path):
    lines = path.read_text().splitlines()
    metadata = dict(line[2:].split("=", 1) for line in lines if line.startswith("# ") and "=" in line)
    rows = list(csv.DictReader((line for line in lines if not line.startswith("#")), delimiter="\t"))
    if len(rows) < 2:
        raise ValueError(f"{path}: at least two samples are required")
    times = [number(row, "uptime_s") for row in rows]
    if any(value is None for value in times) or any(b <= a for a, b in zip(times, times[1:])):
        raise ValueError(f"{path}: sample times must be strictly increasing")
    elapsed = times[-1] - times[0]
    warnings = []
    remaining_charge = metadata.get("current_now_semantics") == "remaining_charge_mah"
    if remaining_charge:
        warnings.append("Ricoh619 current_now is remaining mAh, not current; charge_now was normalized by the recorder.")
    discharging = all(row.get("status") == "Discharging" for row in rows)
    if not discharging:
        warnings.append("Not every sample is discharging; consumption comparison is invalid.")
    methods = {}
    for field, method in (("energy_now", "energy_delta"), ("charge_now", "charge_delta")):
        start, end = number(rows[0], field), number(rows[-1], field)
        if start is None or end is None:
            continue
        drop = start - end
        if drop <= 0:
            warnings.append(f"{field} did not decrease; resolution, duration or gauge corrections prevent a useful estimate.")
            continue
        if field == "energy_now":
            methods[method] = {"average_mw": drop * 3.6 / elapsed, "energy_mwh": drop / 1000.0}
        else:
            values = [number(row, "voltage_now") for row in rows]
            values = [value / 1_000_000 for value in values if value is not None and value > 0]
            average_ma = drop * 3.6 / elapsed
            methods[method] = {"average_ma": average_ma, "charge_mah": drop / 1000.0}
            if remaining_charge:
                methods[method]["gauge_step_mah"] = 1.0
                methods[method]["average_ma_per_gauge_step"] = 3600.0 / elapsed
            if values:
                methods[method]["average_mw"] = average_ma * statistics.mean(values)
                methods[method]["power_is_approximate"] = True
    for field in ("current_now", "current_avg"):
        if field == "current_now" and remaining_charge:
            continue
        readings = []
        for row in rows:
            voltage, current = number(row, "voltage_now"), number(row, field)
            readings.append(abs(current) * voltage / 1_000_000_000 if current is not None and voltage is not None and voltage > 0 else None)
        if any(value is None for value in readings) or not any(value > 0 for value in readings):
            continue
        integral = sum((b - a) * (left + right) / 2 for a, b, left, right in zip(times, times[1:], readings, readings[1:]))
        methods[field] = {"average_mw": integral / elapsed, "sampled_energy_mwh": integral / 3600.0}
    if not methods:
        warnings.append("No usable charge, energy or current readings; only coarse percentage drain is available.")
    first_percent, last_percent = number(rows[0], "capacity"), number(rows[-1], "capacity")
    return {
        "file": str(path), "label": metadata.get("label"), "battery": metadata.get("battery"),
        "duration_seconds": elapsed, "samples": len(rows), "all_discharging": discharging,
        "percentage_points_per_hour": (first_percent - last_percent) * 3600 / elapsed if first_percent is not None and last_percent is not None else None,
        "methods": methods, "warnings": warnings,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("captures", type=Path, nargs="+", help="One capture to summarize, or a Bokheim/Nickel pair to compare")
    args = parser.parse_args()
    if len(args.captures) > 2:
        parser.error("Supply one capture or one pair")
    try:
        captures = [summarize(path) for path in args.captures]
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")
    result = {"captures": captures}
    if len(captures) == 2:
        labels = [capture["label"] or "" for capture in captures]
        workloads = {label.split("-", 1)[-1] for label in labels}
        apps = {label.split("-", 1)[0] for label in labels}
        if len(workloads) != 1 or apps != {"bokheim", "nickel"}:
            parser.error("Comparison requires Bokheim and Nickel captures of the same workload")
        bokheim = next(capture for capture in captures if capture["label"].startswith("bokheim-"))
        nickel = next(capture for capture in captures if capture["label"].startswith("nickel-"))
        if not all(capture["all_discharging"] for capture in captures):
            result["comparison"] = {"error": "Charging or unknown battery status invalidates this comparison"}
        else:
            for method in ("energy_delta", "charge_delta", "current_now", "current_avg"):
                left = bokheim["methods"].get(method, {}).get("average_mw")
                right = nickel["methods"].get(method, {}).get("average_mw")
                if left is not None and right is not None and right > 0:
                    result["comparison"] = {
                        "method": method, "bokheim_average_mw": left, "nickel_average_mw": right,
                        "bokheim_vs_nickel_ratio": left / right,
                        "bokheim_difference_percent": (left / right - 1) * 100,
                        "conditions": "Valid only with matching lighting, wireless settings, workload and gauge units; repeat runs to assess variation.",
                    }
                    break
            else:
                result["comparison"] = {"error": "No common usable power-estimation method; inspect raw readings or measure for longer"}
    print(json.dumps(result, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
