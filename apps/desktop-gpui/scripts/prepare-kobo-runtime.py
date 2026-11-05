#!/usr/bin/env python3
"""Build the pinned Kobo libc and FBInk, including support for pre-3.2 kernels.

Only the final cross-compiler prefix is printed to stdout. Build logs stay in
the task cache. Nothing is installed into the host or firmware system paths.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
SOURCES = ROOT / "apps/desktop-gpui/kobo/sources.json"
CACHE = ROOT / "target/kobo-toolchain-cache"
OUTPUT = ROOT / "target/desktop-gpui-kobo"
FORK = ROOT.parent / "GPUI-Fork"
BASELINE = "-O2 -march=armv7-a -mfpu=vfpv3-d16 -mfloat-abi=hard"


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def run(command, *, cwd=ROOT, environment=None, log=None):
    environment = dict(os.environ) if environment is None else dict(environment)
    # Archive builds must never discover the surrounding application Git repo.
    environment["GIT_CEILING_DIRECTORIES"] = str(CACHE.resolve())
    with (log or OUTPUT / "prepare.log").open("a") as output:
        subprocess.run(command, cwd=cwd, env=environment, stdout=output,
                       stderr=subprocess.STDOUT, check=True)


def unpack(spec):
    archive = CACHE / spec["archive"]
    if not archive.exists():
        temporary = archive.with_suffix(archive.suffix + ".partial")
        subprocess.run(["curl", "--fail", "--location", "--silent", "--show-error", "--retry", "3",
                        spec["url"], "--output", str(temporary)], check=True)
        if digest(temporary) != spec["sha256"]:
            raise RuntimeError(f"Kobo source checksum mismatch: {spec['archive']}")
        temporary.replace(archive)
    if digest(archive) != spec["sha256"]:
        raise RuntimeError(f"Kobo source checksum mismatch: {archive}")
    destination = CACHE / spec["directory"]
    marker = destination / ".bokheim-source-sha256"
    if not marker.is_file() or marker.read_text().strip() != spec["sha256"]:
        destination.mkdir(parents=True, exist_ok=True)
        # Every archive is SHA-256 pinned; preserve the toolchain's symlinks.
        run(["tar", "-xf", str(archive), "-C", str(destination), "--strip-components=1"])
        marker.write_text(spec["sha256"] + "\n")
    return destination


def prepare():
    CACHE.mkdir(parents=True, exist_ok=True)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    specs = json.loads(SOURCES.read_text())
    make_source = unpack(specs["make"])
    make = CACHE / "make-host/bin/make"
    if not make.is_file():
        run(["./configure", "--prefix=" + str(CACHE / "make-host"), "--without-guile"], cwd=make_source)
        run(["make", "-j4"], cwd=make_source)
        run(["make", "install"], cwd=make_source)
    toolchain = unpack(specs["toolchain"])
    source = unpack(specs["glibc"])
    cross = toolchain / "bin/arm-buildroot-linux-gnueabihf"
    old_sysroot = Path(subprocess.check_output([str(cross) + "-gcc", "-print-sysroot"], text=True).strip())
    sysroot = OUTPUT / "sysroot"
    marker = sysroot / ".bokheim-runtime"
    runtime_key = f"{specs['toolchain']['sha256']} {specs['glibc']['sha256']} kernel=2.6.32 {BASELINE}"
    jobs = os.environ.get("CARGO_BUILD_JOBS", "4")
    if not marker.is_file() or marker.read_text() != runtime_key:
        build = CACHE / source.name.replace("-source", "-build")
        build.mkdir(parents=True, exist_ok=True)
        if sysroot.exists():
            shutil.rmtree(sysroot)
        sysroot.mkdir(parents=True, exist_ok=True)
        # Linux headers and GCC helper libraries accompany the custom libc.
        shutil.copytree(old_sysroot / "usr/include", sysroot / "usr/include", dirs_exist_ok=True, symlinks=True)
        configure = [str(source / "configure"), "--prefix=/usr", "--libdir=/lib",
                     "--host=arm-buildroot-linux-gnueabihf", "--build=x86_64-pc-linux-gnu",
                     "--with-headers=" + str(old_sysroot / "usr/include"), "--enable-kernel=2.6.32",
                     "--disable-werror", "CC=" + str(cross) + "-gcc", "CFLAGS=" + BASELINE]
        if not (build / "config.status").exists():
            run(configure, cwd=build, log=OUTPUT / "glibc-configure.log")
        run([str(make), "-j" + jobs], cwd=build, log=OUTPUT / "glibc-build.log")
        run([str(make), "install", "install_root=" + str(sysroot)], cwd=build, log=OUTPUT / "glibc-install.log")
        for library in ["libgcc_s.so.1", "libstdc++.so.6"]:
            path = Path(subprocess.check_output([str(cross) + "-gcc", "-print-file-name=" + library], text=True).strip())
            shutil.copyfile(path.resolve(), sysroot / "lib" / library)
        notes = subprocess.check_output([str(cross) + "-readelf", "-n", str(sysroot / "lib/libc.so.6")], text=True)
        if "ABI: 2.6.32" not in notes:
            raise RuntimeError("Kobo libc does not declare the required Linux 2.6.32 baseline")
        marker.write_text(runtime_key)

    wrappers = OUTPUT / "toolchain/bin"
    wrappers.mkdir(parents=True, exist_ok=True)
    prefix = wrappers / "arm-bokheim-linux-gnueabihf"
    import shlex
    for tool in ["gcc", "g++", "ar", "ranlib", "gcc-ar", "gcc-ranlib", "gcc-nm", "readelf", "strip", "objcopy", "nm"]:
        path = Path(str(prefix) + "-" + tool)
        arguments = [str(cross) + "-" + tool]
        if tool in ["gcc", "g++"]:
            arguments.append("--sysroot=" + str(sysroot))
        path.write_text("#!/bin/sh\nexec " + shlex.join(arguments) + ' "$@"\n')
        path.chmod(0o755)

    fbink = unpack(specs["fbink"])
    for name in ["font8x8", "i2c-tools", "stb"]:
        unpack(specs[name])
    # The application also produces a cdylib; every embedded native object
    # must support shared linking, including the manually compiled shim.
    flags = os.environ.get("KOBO_CFLAGS", BASELINE) + " -fPIC"
    shim = FORK / "crates/gpui_kobo/native/fbink_shim.c"
    fbink_key = runtime_key + flags + digest(SOURCES) + digest(shim) + digest(Path(__file__))
    fbink_marker = fbink / ".bokheim-build"
    if not fbink_marker.is_file() or fbink_marker.read_text() != fbink_key:
        # Upstream distclean resets its Git submodules. Source archives are not
        # Git repositories; use only build-output cleanup and explicitly rebuild
        # the generated i2c staging area.
        run(["make", "clean"], cwd=fbink)
        run(["make", "-C", "i2c-tools", "clean"], cwd=fbink)
        shutil.rmtree(fbink / "libi2c-staged", ignore_errors=True)
        (fbink / "libi2c.built").unlink(missing_ok=True)
        run(["make", "-j" + jobs, "static", "MINIMAL=1", "BITMAP=1", "IMAGE=1",
             "CROSS_TC=" + str(prefix), "CFLAGS=" + flags, "LDFLAGS=-static", "LIBS=-lm",
             "FBINK_VERSION=git-" + specs["fbink"]["revision"][:12]], cwd=fbink, log=OUTPUT / "fbink-build.log")
        run([str(prefix) + "-gcc", *flags.split(), "-std=c11", "-I" + str(fbink), "-c", str(shim),
             "-o", str(fbink / "Release/gpui_fbink_shim.o")])
        run([str(prefix) + "-ar", "rcs", str(fbink / "Release/libgpui_fbink_shim.a"), str(fbink / "Release/gpui_fbink_shim.o")])
        fbink_marker.write_text(fbink_key)
    return prefix


if __name__ == "__main__":
    try:
        print(prepare())
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(f"Kobo runtime preparation failed: {error}. See {OUTPUT}/*.log")
