#!/usr/bin/env python3
"""Release component benchmarks of backend review fixes, using synthetic local data.

Runs production APIs in fresh processes. Baselines reconstruct the former buffered
archive and whole-checkpoint algorithms; this is not a benchmark of an old commit.
Excludes UI, network, browser OPFS, and end-to-end copy/scanner scheduling.
"""
import hashlib
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import tomllib
import shutil
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    for process in ("cargo", "rustc"):
        if subprocess.run(["pgrep", "-x", process], stdout=subprocess.DEVNULL).returncode == 0:
            raise SystemExit(f"Wait for existing {process} before compiling.")
    cache = Path.home() / ".cache" / "bokheim" / "backend-measurements"
    cache.mkdir(parents=True, exist_ok=True)
    filesystem = subprocess.check_output(["stat", "-f", "-c", "%T", str(cache)], text=True).strip()
    if filesystem.startswith(("fuse", "nfs", "cifs")):
        raise SystemExit(f"Local storage required; found {filesystem}")
    root = Path(tempfile.mkdtemp(prefix="run-", dir=cache))
    dependencies = {
        "audiobook-archive": "shared/audiobook-archive",
        "library-files": "client/library-backend/crates/library-files",
        "library-model": "client/library-model",
        "library-registry": "client/library-management/crates/registry",
        "sync-common": "shared/sync-common",
    }
    manifest = root / "Cargo.toml"
    manifest.write_text('[package]\nname="backend-measurements"\nversion="0.1.0"\nedition="2021"\n[workspace]\n[[bin]]\nname="backend-measurements"\npath=' + json.dumps(str(Path(__file__).with_suffix('.rs'))) + '\n[dependencies]\n' + ''.join(f'{name}={{path={json.dumps(str(ROOT / path))}}}\n' for name,path in dependencies.items()) + 'blake3="1"\nserde_json="1"\ntokio={version="1",features=["rt-multi-thread","time"]}\n[patch.crates-io]\nzip={path=' + json.dumps(str(ROOT / 'vendor/zip')) + '}\n')
    patches = tomllib.loads((ROOT / "Cargo.toml").read_text())["patch"]["crates-io"]
    with manifest.open("a") as output:
        for name in ("mobi", "async_zip", "usvg", "pdfium-render"):
            output.write(f'{name}={{path={json.dumps(str(ROOT / patches[name]["path"]))}}}\n')
    shutil.copyfile(ROOT / "Cargo.lock", root / "Cargo.lock")
    target = cache / "target"
    print(f"Results: {root / 'results.json'}", flush=True)
    subprocess.run(["cargo", "build", "--offline", "--release", "--manifest-path", str(manifest), "--target-dir", str(target)], cwd=ROOT, check=True)
    binary = target / "release/backend-measurements"
    result = {"started_utc":datetime.now(timezone.utc).isoformat(), "cpu_count":os.cpu_count(), "filesystem":filesystem, "platform":platform.platform(), "profile":"release, opt-level=3, default Cargo codegen/lto", "notes":__doc__, "binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(), "samples":[], "summaries":[]}
    def run(component, mode, fixture, size, count=0):
        samples=[]
        for repeat in range(3):
            work = fixture
            if component == 'checkpoint':
                work = fixture / f'{mode}-{repeat}'
                work.mkdir(parents=True)
            output = subprocess.check_output([str(binary),component,mode,str(work),str(count)],text=True)
            sample=json.loads(output)
            if component == "review" and sample["present"] != count:
                raise RuntimeError("Presence check did not find every fixture")
            if component == "checkpoint" and sample["saved"] != count:
                raise RuntimeError("Checkpoint count differs from fixture")
            sample.update(size=size,repeat=repeat+1)
            result['samples'].append(sample)
            samples.append(sample)
            print(json.dumps(sample),flush=True)
        summary={"component":component,"mode":mode,"size":size}
        for key in ('elapsed_ms','peak_kib'):
            summary[key]=statistics.median(s[key] for s in samples)
        result['summaries'].append(summary)
        (root/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    block=os.urandom(1024*1024)
    for mib in (32,256):
        fixture=root/f'archive-{mib}'
        fixture.mkdir()
        with zipfile.ZipFile(fixture/'source.zip','w',compression=zipfile.ZIP_STORED,allowZip64=True) as archive:
            with archive.open('01.mp3','w',force_zip64=True) as track:
                for _ in range(mib): track.write(block)
        for mode in ('buffered','streamed'): run('archive',mode,fixture,mib)
        def digest(path):
            with path.open('rb') as file: return hashlib.file_digest(file,'sha256').hexdigest()
        if digest(fixture/'buffered.zip') != digest(fixture/'streamed.zip'):
            raise RuntimeError('Archive outputs differ')
        for path in fixture.iterdir(): path.unlink()
    for mib in (1,16):
        fixture=root/f'review-{mib}'
        fixture.mkdir()
        for i in range(16):
            with (fixture/f'book-{i}.epub').open('wb') as file:
                for _ in range(mib): file.write(block)
        for mode in ('checksum','metadata'): run('review',mode,fixture,mib,16)
        for path in fixture.glob('book-*.epub'): path.unlink()
    for count in (100,1000):
        fixture=root/f'checkpoint-{count}'
        for mode in ('whole','member'): run('checkpoint',mode,fixture,count,count)
    print(json.dumps(result['summaries'],indent=2))


if __name__ == '__main__':
    main()
