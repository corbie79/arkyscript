#!/usr/bin/env python3
"""Build and test the C++20 binder against the pinned, unmodified LuaJIT."""
import argparse
import os
import shlex
import subprocess
import sys

from build_common import ROOT, dependencies, find_tool, run


TEST_SOURCES = tuple(ROOT / "tests" / "binder" / name for name in
                     ("arg.cpp", "object.cpp", "value.cpp", "binder.cpp"))
EXAMPLE_SOURCES = tuple(ROOT / "examples" / name for name in
                        ("cpp_binder.cpp", "cpp_class.cpp", "cpp_traits.cpp"))
SOURCES = TEST_SOURCES + EXAMPLE_SOURCES


def native(revision, sanitize, sources=SOURCES):
    engine = ROOT / "build" / "native" / revision / "src"
    library = engine / "libluajit.a"
    if not library.is_file():
        raise RuntimeError("Pinned native LuaJIT is missing; run make setup first")
    destination = ROOT / "build" / "binder" / ("sanitize" if sanitize else "native") / revision
    destination.mkdir(parents=True, exist_ok=True)
    compiler = shlex.split(os.environ.get("CXX", "c++"))
    flags = ["-std=c++20", "-Wall", "-Wextra", "-Wpedantic", "-Werror", "-g",
             "-I", str(ROOT), "-I", str(engine)]
    if sanitize:
        flags += ["-O1", "-fsanitize=address,undefined", "-fno-omit-frame-pointer"]
    else:
        flags += ["-O2"]
    libraries = [str(library)]
    if sys.platform.startswith("linux"):
        libraries += ["-ldl", "-lm"]
    for source in sources:
        binary = destination / source.stem
        run(*compiler, *flags, source, *libraries, "-o", binary)
        run(binary, cwd=ROOT)
    print(f"Native binder validation passed: {destination}", flush=True)


def windows(revision, sources=SOURCES):
    from build_windows import check_pe, prepare_sdk

    engine = ROOT / "build" / "windows-x64" / revision
    library = engine / "lua51-static.lib"
    if not library.is_file():
        raise RuntimeError("Pinned Windows LuaJIT is missing; run make windows first")
    sdk, _ = prepare_sdk()
    compiler = find_tool("clang-cl", os.environ.get("CLANG_CL"))
    linker = find_tool("lld-link", os.environ.get("LLD_LINK"))
    destination = ROOT / "build" / "binder" / "windows-x64" / revision
    destination.mkdir(parents=True, exist_ok=True)
    includes = [sdk / part for part in ("crt/include", "sdk/include/ucrt", "sdk/include/um", "sdk/include/shared")]
    libraries = [sdk / part for part in ("crt/lib/x86_64", "sdk/lib/ucrt/x86_64", "sdk/lib/um/x86_64")]
    for source in sources:
        obj = destination / (source.stem + ".obj")
        binary = destination / (source.stem + ".exe")
        run(compiler, "--target=x86_64-pc-windows-msvc", "/nologo", "/c", "/std:c++20", "/EHsc",
            "/permissive-", "/W4", "/WX", "/O2", "/MT", f"/I{ROOT}", f"/I{engine / 'include'}",
            *[f"/imsvc{path}" for path in includes], f"/Fo{obj}", "--", source)
        run(linker, "/nologo", "/machine:x64", "/subsystem:console", f"/out:{binary}",
            *[f"/libpath:{path}" for path in libraries], obj, library, "kernel32.lib")
        check_pe(binary)
    print(f"Windows x64 C++20 build/PE validation passed: {destination}\n"
          "Run the generated EXEs on Windows for runtime validation.", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--sanitize", action="store_true", help="Run native ASan/UBSan checks")
    mode.add_argument("--windows", action="store_true", help="Cross-build with the pinned xwin SDK")
    parser.add_argument("--examples", action="store_true", help="Build only the runnable C++ examples")
    args = parser.parse_args()
    revision = dependencies()["luajit"]["revision"]
    sources = EXAMPLE_SOURCES if args.examples else SOURCES
    if args.windows:
        windows(revision, sources=sources)
    else:
        native(revision, args.sanitize, sources=sources)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(f"binder-test: {error}")
