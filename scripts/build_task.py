#!/usr/bin/env python3
"""Build the Tynex task module beside the unmodified LuaJIT engine."""
import argparse
import hashlib
import os
import shlex
import shutil
import subprocess
import sys

from build_common import ROOT, dependencies, find_tool, run, write_json


def record_build(target, revision):
    sources = sorted((ROOT / "runtime/task").glob("*")) + sorted((ROOT / "binder").glob("*.h")) + sorted((ROOT / "binder").glob("*.inl"))
    write_json(target.parent / "task-build-info.json", {
        "luajit_revision": revision,
        "module": target.name,
        "sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
        "sources": {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in sources if path.is_file()},
    })


def native(revision):
    engine = ROOT / "build" / "native" / revision / "src"
    if not (engine / "luajit").is_file():
        raise RuntimeError("Build the native engine first: make setup")
    target = engine / "tynex_task.so"
    compiler = shlex.split(os.environ.get("CXX", "c++"))
    shared = ["-bundle", "-undefined", "dynamic_lookup"] if sys.platform == "darwin" else ["-shared"]
    run(*compiler, "-std=c++20", "-O2", "-g", "-Wall", "-Wextra", "-Wpedantic", "-Werror",
        "-fPIC", "-pthread", *shared, "-I", ROOT, "-I", engine,
        ROOT / "runtime/task/lua_task.cpp", "-o", target)
    record_build(target, revision)
    return target


def windows(revision):
    from build_windows import check_pe, prepare_sdk

    engine = ROOT / "build/windows-x64" / revision
    if not (engine / "lua51.lib").is_file():
        raise RuntimeError("Build the Windows engine first: make windows")
    sdk, _ = prepare_sdk()
    compiler = find_tool("clang-cl", os.environ.get("CLANG_CL"))
    linker = find_tool("lld-link", os.environ.get("LLD_LINK"))
    obj = engine / "tynex_task.obj"
    target = engine / "tynex_task.dll"
    includes = [sdk / part for part in ("crt/include", "sdk/include/ucrt", "sdk/include/um", "sdk/include/shared")]
    libraries = [sdk / part for part in ("crt/lib/x86_64", "sdk/lib/ucrt/x86_64", "sdk/lib/um/x86_64")]
    run(compiler, "--target=x86_64-pc-windows-msvc", "/nologo", "/c", "/std:c++20", "/EHsc",
        "/permissive-", "/W4", "/WX", "/O2", "/MT", "/DLUA_BUILD_AS_DLL",
        f"/I{ROOT}", f"/I{engine / 'include'}", *[f"/imsvc{path}" for path in includes],
        f"/Fo{obj}", "--", ROOT / "runtime/task/lua_task.cpp")
    run(linker, "/nologo", "/machine:x64", "/dll", f"/out:{target}",
        *[f"/libpath:{path}" for path in libraries], obj, engine / "lua51.lib", "kernel32.lib")
    check_pe(target, dll=True)
    obj.unlink()
    record_build(target, revision)
    archive = shutil.make_archive(str(engine.parent / f"tynex-windows-x64-{revision}"), "zip", engine)
    print(f"Tynex runtime ZIP (engine + Task): {archive}", flush=True)
    return target


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--windows", action="store_true")
    args = parser.parse_args()
    revision = dependencies()["luajit"]["revision"]
    print(windows(revision) if args.windows else native(revision))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(f"task-build: {error}")
