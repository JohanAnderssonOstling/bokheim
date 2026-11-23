#!/usr/bin/env python3
"""Prepare verified ARM64 speech libraries and offline voice assets for Android."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil

from prepare import ASSET_SHA256 as VOICE_SHA256, prepare

REQUIRED = ("libsherpa-onnx-c-api.so", "libonnxruntime.so")


def prepare_minimal(destination, ndk, sdk, source_work, jobs, offline):
    """Install matching reduced libraries and the converted FP32 Alba graph."""
    if ndk is None or sdk is None:
        raise RuntimeError("Reduced Android speech requires an Android NDK and SDK")
    native = runpy.run_path(str(Path(__file__).with_name("build_native.py")))
    libraries = native["prepare_native"](source_work, jobs, offline, ndk=ndk, android_sdk=sdk)
    bundle = libraries.parent
    receipt = json.loads((bundle / "receipt.json").read_text())
    converted = receipt["operator_analysis"]["converted_models"]["alba/en_GB-alba-medium.ort"]
    destination = Path(destination)
    voice = destination / "assets/tts/alba"
    model = voice / "en_GB-alba-medium.ort"
    marker = voice / "voice.sha256"
    identity = hashlib.sha256(f"{VOICE_SHA256}:{converted['sha256']}:{receipt['fingerprint']}".encode()).hexdigest()
    if not (marker.is_file() and marker.read_text().strip() == identity
            and model.is_file() and native["digest"](model) == converted["sha256"]):
        prepare(voice, offline=offline)
        shutil.copy2(bundle / "models/alba/en_GB-alba-medium.ort", model)
        marker.write_text(identity + "\n")
    (voice / "en_GB-alba-medium.onnx").unlink(missing_ok=True)
    installed = destination / "jniLibs/arm64-v8a"
    installed.mkdir(parents=True, exist_ok=True)
    for name in REQUIRED:
        shutil.copy2(libraries / name, installed / name)
    shutil.copy2(bundle / "LICENSE", installed / "LICENSE")
    shutil.copy2(bundle / "receipt.json", installed / "SOURCE.json")
    shutil.copy2(bundle / "LICENSE", destination / "assets/tts/RUNTIME-LICENSE.txt")
    shutil.copy2(bundle / "receipt.json", destination / "assets/tts/RUNTIME-SOURCE.json")
    # A legacy preparation must never reuse these libraries with ONNX assets.
    (installed / ".archive-sha256").unlink(missing_ok=True)
    george = destination / "assets/tts/george"
    if george.exists():
        shutil.rmtree(george)
    return installed


def prepare_android(destination, offline=False, from_source=False, onnxruntime_aar=None, ndk=None, source_work=None, jobs=None, sdk=None):
    jobs = jobs if jobs is not None else os.cpu_count() or 1
    if not from_source:
        return prepare_minimal(destination, ndk, sdk, source_work, jobs, offline)
    destination = Path(destination)
    libraries = destination / "jniLibs/arm64-v8a"
    if onnxruntime_aar is None or ndk is None:
        raise RuntimeError("F-Droid speech requires --onnxruntime-aar and --ndk")
    prepare_source = runpy.run_path(str(Path(__file__).with_name("build_android_source.py")))["prepare_source"]
    prepare_source(libraries, onnxruntime_aar, ndk, source_work, jobs, offline)
    voice_archive = (Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) /
        "bokheim/tts/vits-piper-en_GB-alba-medium.tar.bz2")
    prepare(destination / "assets/tts/alba", offline=offline, archive=voice_archive)
    (destination / "assets/tts/alba/en_GB-alba-medium.ort").unlink(missing_ok=True)
    (destination / "assets/tts/alba/voice.sha256").write_text(VOICE_SHA256 + "\n")
    license_directory = Path(__file__).with_name("licenses")
    for name in ("alba-model-card.txt", "alba-provenance.json", "CC-BY-4.0.txt"):
        shutil.copy2(license_directory / name, destination / "assets/tts/alba" / name)
    # Remove the previously bundled voice from reused build directories.
    george = destination / "assets/tts/george"
    if george.exists():
        shutil.rmtree(george)
    return libraries


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--from-source", action="store_true")
    parser.add_argument("--onnxruntime-aar", type=Path)
    parser.add_argument("--ndk", type=Path)
    parser.add_argument("--source-work", type=Path)
    parser.add_argument("--sdk", type=Path)
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    if not args.from_source and (args.ndk is None or args.sdk is None):
        parser.error("Reduced Android speech requires --ndk and --sdk")
    print(prepare_android(args.destination, args.offline, args.from_source, args.onnxruntime_aar,
        args.ndk, args.source_work, args.jobs, args.sdk))
