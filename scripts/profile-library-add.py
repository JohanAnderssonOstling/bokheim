#!/usr/bin/env python3
"""Summarize opt-in library_add_latency logs; times are milliseconds."""
import argparse
import re
from pathlib import Path


def summarize(text):
    operations, library_events, backend_events, open_events = {}, {}, {}, {}
    for line in text.splitlines():
        if "library_add_latency" not in line:
            continue
        fields = dict(re.findall(r"(\w+)=([^\s,]+)", line))
        stage = fields.get("stage")
        if not stage:
            continue
        if "ui_trace" in fields:
            op = operations.setdefault(fields["ui_trace"], {"events": {}})
            op["events"][stage] = float(fields["at_ms"])
            if "library_id" in fields:
                op["library"] = fields["library_id"]
        elif "backend_trace" in fields:
            backend = backend_events.setdefault(fields["backend_trace"], {"libraries": set(), "events": {}})
            if "library_id" in fields:
                backend["libraries"].add(fields["library_id"])
            if "elapsed_ms" in fields:
                backend["events"][stage] = float(fields["elapsed_ms"])
        elif "open_trace" in fields:
            scope = open_events.setdefault(fields["open_trace"], {"library": fields["library_id"], "scope": fields["scope"], "phases_ms": {}})
            scope["phases_ms"][stage] = float(fields["phase_ms"])
            scope["elapsed_ms"] = float(fields["elapsed_ms"])
        elif "library_id" in fields and "at_ms" in fields:
            library_events.setdefault(fields["library_id"], []).append((stage, float(fields["at_ms"])))
    output = []
    for identifier, op in operations.items():
        events = dict(op["events"])
        start = events.get("picker_returned")
        if start is None:
            continue
        library = op.get("library")
        # Include early list-notification selection, before the response/attach.
        for stage, timestamp in library_events.get(library, []):
            if timestamp >= start:
                events.setdefault(stage, timestamp)
        result = {"trace": identifier, "library": library}
        for stage in ("backend_response_received", "list_refresh_received", "selection_updated", "selected_frame_completed", "home_response_received", "home_result_installed", "home_frame_completed", "home_load_failed", "backend_request_failed"):
            if stage in events:
                result[stage + "_ms"] = round(events[stage] - start, 3)
        if "backend_response_received" in events and "backend_request_sent" in events:
            request_ms = events["backend_response_received"] - events["backend_request_sent"]
            result["request_round_trip_ms"] = round(request_ms, 3)
            candidates = [b for b in backend_events.values() if library in b["libraries"]]
            if len(candidates) == 1 and "backend_done" in candidates[0]["events"]:
                work = candidates[0]["events"]["backend_done"]
                result["backend_work_ms"] = work
                result["queue_transport_and_scheduling_ms"] = round(request_ms - work, 3)
        scopes = [scope for scope in open_events.values() if scope["library"] == library and ("cache_hit" not in scope["phases_ms"] or scope["elapsed_ms"] >= 1.0)]
        if scopes:
            result["open_scopes"] = scopes
        output.append(result)
    return output


if __name__ == "__main__":
    import json
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    args = parser.parse_args()
    print(json.dumps(summarize(args.log.read_text()), indent=2))
