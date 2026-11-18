#!/usr/bin/env python3
"""Run the standalone wire allocation profiler; CSV is printed to stdout.

Uses local Cargo storage and cached dependencies. No database or service access.
Do not run concurrently with another Rust compilation.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    for process in ("cargo", "rustc"):
        if subprocess.run(["pgrep", "-x", process], stdout=subprocess.DEVNULL).returncode == 0:
            raise SystemExit(f"Wait for the existing {process} process before profiling.")
    cache = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    target = cache / "bokheim/builds/wire-allocations"
    # Reject remote build storage even if the user's cache directory is redirected.
    existing = target.resolve()
    while not existing.exists():
        existing = existing.parent
    filesystem = subprocess.check_output(["stat", "-f", "-c", "%T", str(existing)], text=True).strip()
    if filesystem.startswith(("fuse", "nfs", "cifs")):
        raise SystemExit(f"Allocation profiling requires local build storage, found {filesystem}.")
    dependencies = {
        "library-replica": "client/library-backend/crates/library-replica",
        "book-model": "shared/book-model",
        "pdf-view-common": "shared/pdf-view-common",
        "sync-common": "shared/sync-common",
    }
    with tempfile.TemporaryDirectory(prefix="bokheim-wire-allocation-profile-") as directory:
        manifest = Path(directory) / "Cargo.toml"
        manifest.write_text(
            '[package]\nname = "bokheim-wire-allocation-profile"\nversion = "0.1.0"\nedition = "2021"\n'
            '[workspace]\n[[bin]]\nname = "wire-allocations"\npath = '
            + json.dumps(str(Path(__file__).with_suffix(".rs")))
            + "\n[dependencies]\n"
            + "".join(f'{name} = {{ path = {json.dumps(str(ROOT / path))} }}\n' for name, path in dependencies.items())
        )
        subprocess.run(
            ["cargo", "run", "--offline", "--release", "--manifest-path", str(manifest), "--target-dir", str(target)],
            check=True, cwd=ROOT,
        )


if __name__ == "__main__":
    main()
