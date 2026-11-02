#!/usr/bin/env python3
"""Build reduced, static Linux speech libraries without editing dependency sources."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import runpy
import shlex
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
LIBRARIES = ("sherpa-onnx-c-api", "sherpa-onnx-core", "kaldi-decoder-core",
             "sherpa-onnx-kaldifst-core", "sherpa-onnx-fstfar", "sherpa-onnx-fst",
             "kaldi-native-fbank-core", "kissfft-float", "piper_phonemize",
             "espeak-ng", "ucd", "onnxruntime", "ssentencepiece_core")


def digest(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def run(command, **kwargs):
    print("+ " + " ".join(map(str, command)), flush=True)
    subprocess.run(list(map(str, command)), check=True, **kwargs)


def local_directory(path):
    path = Path(path).resolve()
    existing = path
    while not existing.exists():
        existing = existing.parent
    filesystem = subprocess.check_output(["findmnt", "-T", str(existing), "-n", "-o", "FSTYPE"], text=True).strip()
    if filesystem in ("fuse.sshfs", "sshfs"):
        raise RuntimeError(f"Speech builds require local storage, not SSHFS: {path}")
    path.mkdir(parents=True, exist_ok=True)
    return path


def bundle_archives(archives, output):
    # MRI ADDLIB preserves members with duplicate names. Extracting .o files
    # into one directory would silently overwrite some runtime objects.
    archiver = shutil.which("llvm-ar")
    if not archiver:
        raise RuntimeError("llvm-ar is required to bundle static speech libraries")
    with tempfile.TemporaryDirectory(dir=output.parent) as temporary:
        temporary = Path(temporary)
        commands = ["CREATE libonnxruntime.a"]
        for index, archive in enumerate(archives):
            name = f"input-{index}.a"
            (temporary / name).symlink_to(archive.resolve(strict=True))
            commands.append(f"ADDLIB {name}")
        commands += ["SAVE", "END", ""]
        run([archiver, "-M"], input="\n".join(commands), text=True, cwd=temporary)
        strip_debug(temporary / "libonnxruntime.a")
        (temporary / "libonnxruntime.a").replace(output)


def strip_debug(archive):
    strip = shutil.which("llvm-strip")
    if not strip:
        raise RuntimeError("llvm-strip is required to remove native archive debug information")
    run([strip, "--strip-debug", archive])


def prepare_native(work=None, jobs=1, offline=False):
    if sys.platform != "linux" or platform.machine() not in ("x86_64", "AMD64"):
        raise RuntimeError("Reduced speech source builds currently support x86-64 Linux only")
    if jobs < 1:
        raise ValueError("Speech compiler jobs must be positive")
    pins = json.loads((HERE / "native-source.json").read_text())
    work = local_directory(work or ROOT / ".tmp/speech-reduced-native")
    local_directory(os.environ.get("BOKHEIM_LINUX_SDK", ROOT / ".linux-toolchain"))
    environment = runpy.run_path(str(ROOT / "scripts/linux/toolchain.py"))["build_environment"](offline=offline)
    sdk = Path(environment["BOKHEIM_LINUX_SDK"])
    local_directory(sdk)
    environment.update(TMPDIR=str(local_directory(work / "tmp")), CMAKE_BUILD_PARALLEL_LEVEL=str(jobs))
    inputs = [HERE / name for name in ("native-source.json", "native-operators.py", "native-archives.cmake", "native-cxx-compat.h",
                                      "build_native.py", "build_android_source.py", "source.json", "source-dependencies.json")]
    inputs += [ROOT / "scripts/linux/toolchain.json", ROOT / "scripts/linux/compiler.py"]
    fingerprint = hashlib.sha256("".join(digest(path) for path in inputs).encode()).hexdigest()
    destination = ROOT / "target/native-speech" / fingerprint
    local_directory(destination.parent)
    marker = destination / "receipt.json"
    if marker.is_file():
        cached = json.loads(marker.read_text())
        if cached.get("fingerprint") == fingerprint and all(
                (destination / name).is_file() and digest(destination / name) == checksum
                for name, checksum in cached["files"].items()):
            return destination / "lib"
    required_gib = 2 if (work / "onnxruntime-build/Release/CMakeCache.txt").is_file() else 8
    if shutil.disk_usage(work).free < required_gib * 1024**3:
        raise RuntimeError(f"The native speech source build needs at least {required_gib} GiB of free local space: {work}")

    runtime = pins["onnxruntime"]
    source = work / "onnxruntime-source"
    if not source.exists():
        if offline:
            raise RuntimeError(f"ONNX Runtime source is unavailable offline: {source}")
        run(["git", "clone", "--depth", "1", "--branch", runtime["tag"], "--filter=blob:none", runtime["repository"], source])
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip()
    modified = subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=source, text=True)
    if revision != runtime["commit"] or modified.strip():
        raise RuntimeError("ONNX Runtime must be the pinned, unmodified source checkout")
    if not (source / "cmake/external/onnx/CMakeLists.txt").is_file():
        if offline:
            raise RuntimeError("The pinned ONNX source submodule is unavailable offline")
        run(["git", "submodule", "update", "--init", "--depth", "1", "cmake/external/onnx"], cwd=source)

    python = work / "python/bin/python"
    if not python.is_file():
        run([sys.executable, "-m", "venv", work / "python"])
    packages = pins["operator_tools"]
    versions = "import onnx,onnxruntime; assert (onnx.__version__,onnxruntime.__version__)==" + repr((packages["onnx"], packages["onnxruntime"]))
    if subprocess.run([python, "-c", versions], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
        if offline:
            raise RuntimeError("Pinned ONNX analysis packages must be installed before an offline build")
        run([python, "-m", "pip", "install", "--disable-pip-version-check", *[f"{name}=={version}" for name, version in packages.items()]], env=environment)
    models = ROOT / "target/release/tts"
    if not all((models / name).is_file() and digest(models / name) == checksum
               for name, checksum in pins["models"].items()):
        # Do not replace an existing release's ORT assets while preparing a
        # future runtime. A failed native build must leave that release usable.
        models = work / "source-models"
        runpy.run_path(str(HERE / "prepare.py"))["prepare_all"](models, offline=offline)
    operators = work / "operators"
    run([python, HERE / "native-operators.py", "--models", models, "--output", operators], env=environment)
    cmake = [f"CMAKE_C_COMPILER={sdk}/bin/cc", f"CMAKE_CXX_COMPILER={sdk}/bin/c++",
             f"CMAKE_ASM_COMPILER={sdk}/bin/cc", f"CMAKE_AR={shutil.which('llvm-ar')}",
             f"CMAKE_RANLIB={shutil.which('llvm-ranlib')}",
             "CMAKE_C_FLAGS=-g0 -ffunction-sections -fdata-sections",
             "CMAKE_CXX_FLAGS=-g0 -ffunction-sections -fdata-sections -include " + shlex.quote(str(HERE / "native-cxx-compat.h")),
             "CMAKE_EXE_LINKER_FLAGS=-Wl,--gc-sections",
             "CMAKE_LINK_DEPENDS_USE_LINKER=OFF",
             "FETCHCONTENT_TRY_FIND_PACKAGE_MODE=NEVER"]
    runtime_build = work / "onnxruntime-build"
    command = [python, source / "tools/ci_build/build.py", "--build_dir", runtime_build,
               "--config", "Release", "--update", "--parallel", str(jobs),
               "--skip_tests", "--skip_submodule_sync", "--skip_pip_install",
               "--disable_ml_ops", "--disable_generation_ops", "--compile_no_warning_as_error",
               "--minimal_build", "--disable_rtti", "--disable_types", "float8", "float4", "sparsetensor", "optional", "string",
               "--enable_reduced_operator_type_support",
               "--include_ops_by_config", operators / "required_operators.config",
               "--cmake_generator", "Ninja", "--targets", "bokheim-onnxruntime-static",
               "--cmake_extra_defines", *cmake, "onnxruntime_BUILD_UNIT_TESTS=OFF", "protobuf_WITH_ZLIB=OFF",
               "onnxruntime_DISABLE_EXTERNAL_INITIALIZERS=ON",
               f"CMAKE_PROJECT_INCLUDE={HERE}/native-archives.cmake"]
    if offline:
        command += ["FETCHCONTENT_FULLY_DISCONNECTED=ON"]
    # Upstream rewrites its generated reduction headers even when identical.
    # Preserve unchanged build-file timestamps so updating our orchestration
    # does not force every CPU kernel to compile again.
    generated = runtime_build / "Release/op_reduction.generated"
    previous = {path: (digest(path), path.stat()) for path in generated.rglob("*") if path.is_file()}
    run(command, env=environment, cwd=ROOT)
    for path, (checksum, status) in previous.items():
        if path.is_file() and digest(path) == checksum:
            os.utime(path, ns=(status.st_atime_ns, status.st_mtime_ns))
    command[command.index("--update")] = "--build"
    run(command, env=environment, cwd=ROOT)
    manifest = runtime_build / "Release/bokheim-archives.txt"
    archives = list(dict.fromkeys(Path(line) for line in manifest.read_text().splitlines() if line))
    if not archives or not any(path.name == "libonnxruntime_session.a" for path in archives):
        raise RuntimeError("The ONNX archive manifest is missing the runtime session library")
    runtime_lib = work / "onnxruntime-lib"
    runtime_lib.mkdir(exist_ok=True)
    # The SDK can emit debug information even for CMake Release builds.
    # Strip each input first to avoid duplicating it in the merged archive.
    for archive in archives:
        strip_debug(archive)
    bundle_archives(archives, runtime_lib / "libonnxruntime.a")

    sherpa_work = work / "sherpa"
    helpers = runpy.run_path(str(HERE / "build_android_source.py"))
    sherpa = helpers["prefetch_sources"](sherpa_work, offline)
    dependency_flags = helpers["dependency_sources"](sherpa_work)
    environment.update(SHERPA_ONNXRUNTIME_LIB_DIR=str(runtime_lib),
                       SHERPA_ONNXRUNTIME_INCLUDE_DIR=str(source / "include/onnxruntime/core/session"))
    flags = [f"-D{definition}" for definition in cmake]
    flags += ["-DCMAKE_BUILD_TYPE=Release", "-DBUILD_SHARED_LIBS=OFF", "-DSHERPA_ONNX_ENABLE_TTS=ON",
              "-DSHERPA_ONNX_ENABLE_C_API=ON", "-DSHERPA_ONNX_LINK_LIBSTDCPP_STATICALLY=OFF",
              "-DSHERPA_ONNX_BUILD_C_API_EXAMPLES=OFF"]
    flags += [f"-DSHERPA_ONNX_ENABLE_{name}=OFF" for name in
              ("PORTAUDIO", "WEBSOCKET", "BINARY", "TESTS", "CHECK", "SPEAKER_DIARIZATION", "PYTHON", "JNI")]
    build = sherpa_work / "build"
    run(["cmake", "-S", sherpa, "-B", build, "-G", "Ninja", *flags, *dependency_flags], env=environment)
    run(["cmake", "--build", build, "--target", "sherpa-onnx-c-api", "--parallel", str(jobs)], env=environment)

    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent) as staging:
        staging = Path(staging)
        (staging / "lib").mkdir()
        for name in LIBRARIES:
            filename = f"lib{name}.a"
            path = runtime_lib / filename if name == "onnxruntime" else build / "lib" / filename
            if not path.is_file():
                raise RuntimeError(f"Missing native speech archive: {path}")
            shutil.copy2(path, staging / "lib" / filename)
            if name != "onnxruntime":
                strip_debug(staging / "lib" / filename)
        licenses = helpers["notices"]([("sherpa-onnx", sherpa), ("onnxruntime", source)]
                                      + [("sherpa-dependencies/" + p.name, p) for p in sorted((build / "_deps").glob("*-src"))]
                                      + [("ort-dependencies/" + p.name, p) for p in sorted((runtime_build / "Release/_deps").glob("*-src"))])
        (staging / "LICENSE").write_bytes(licenses)
        shutil.copy2(operators / "required_operators.config", staging / "required_operators.config")
        shutil.copytree(operators / "models", staging / "models")
        provenance = {"fingerprint": fingerprint, "target": pins["target"], "glibc": pins["glibc"],
                      "onnxruntime": runtime, "sherpa": json.loads((HERE / "source.json").read_text())["sherpa_commit"],
                      "models": pins["models"], "operator_analysis": json.loads((operators / "operators.json").read_text()),
                      "runtime_features": {"rtti": False, "operator_type_reduction": True, "minimal_build": True,
                                           "model_format": "ORT", "external_initializers": False,
                                           "disabled_types": ["float8", "float4", "sparsetensor", "optional", "string"]},
                      "files": {str(path.relative_to(staging)): digest(path) for path in staging.rglob("*") if path.is_file()}}
        (staging / "receipt.json").write_text(json.dumps(provenance, indent=2) + "\n")
        if destination.exists():
            shutil.rmtree(destination)
        staging.rename(destination)
    return destination / "lib"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", type=Path)
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    print(prepare_native(args.work, args.jobs, args.offline))
