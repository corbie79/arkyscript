#!/usr/bin/env python3
"""Build unpatched LuaJIT for Windows x64 using xwin and native LLVM tools."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile

from build_common import (ROOT, dependencies, find_tool, output, publish_tree,
                          repository, run, snapshot, write_json)


def sdk_root(config):
    return ROOT / ".deps" / "xwin" / f"{config['arch']}-{config['sdk_version']}-{config['crt_version']}"


def prepare_sdk():
    config = json.loads((ROOT / "windows-toolchain.json").read_text())
    xwin = find_tool("xwin", os.environ.get("XWIN"))
    if output(xwin, "--version") != f"xwin {config['xwin_version']}":
        raise RuntimeError(f"Install xwin {config['xwin_version']} from windows-toolchain.json")
    sdk = sdk_root(config)
    marker = sdk / ".tynex-sdk.json"
    legacy_marker = sdk / ".tljit-sdk.json"
    if not marker.exists() and legacy_marker.is_file() and json.loads(legacy_marker.read_text()) == config:
        write_json(marker, config)
    if not marker.is_file() or json.loads(marker.read_text()) != config:
        run(xwin, "--accept-license", "--arch", config["arch"],
            "--variant", config["variant"], "--sdk-version", config["sdk_version"],
            "--crt-version", config["crt_version"],
            "--cache-dir", ROOT / ".deps" / "xwin-cache", "splat", "--output", sdk)
        write_json(marker, config)
    for name in ("crt/include", "sdk/include/ucrt", "sdk/include/um",
                 "sdk/include/shared", "crt/lib/x86_64", "sdk/lib/ucrt/x86_64", "sdk/lib/um/x86_64"):
        if not (sdk / name).is_dir():
            raise RuntimeError(f"Incomplete xwin SDK: {sdk / name}")
    return sdk, config


def library_sources(source):
    # Follow upstream's registration order, including future added libraries.
    recipe = (source / "msvcbuild.bat").read_text()
    match = re.search(r"^@set ALL_LIB=(.+)$", recipe, re.MULTILINE)
    if not match:
        raise RuntimeError("Upstream msvcbuild.bat ALL_LIB changed; review the build adapter")
    names = match.group(1).strip().split()
    if not names or any(not re.fullmatch(r"lib_[a-z0-9_]+\.c", n) or not (source / n).is_file() for n in names):
        raise RuntimeError("Unsupported upstream ALL_LIB definition; review the build adapter")
    return names


def check_pe(path, dll=False):
    data = path.read_bytes()
    if len(data) < 64 or data[:2] != b"MZ":
        raise RuntimeError(f"Not a Windows PE file: {path}")
    offset = struct.unpack_from("<I", data, 60)[0]
    if len(data) < offset + 26 or data[offset:offset + 4] != b"PE\0\0":
        raise RuntimeError(f"Invalid PE header: {path}")
    machine = struct.unpack_from("<H", data, offset + 4)[0]
    characteristics = struct.unpack_from("<H", data, offset + 22)[0]
    magic = struct.unpack_from("<H", data, offset + 24)[0]
    if machine != 0x8664 or magic != 0x20b or bool(characteristics & 0x2000) != dll:
        raise RuntimeError(f"Expected Windows x64 {'DLL' if dll else 'EXE'}: {path}")


def build_windows(revision=None):
    if struct.calcsize("P") != 8 or sys.platform == "win32":
        raise RuntimeError("This cross-build requires a 64-bit macOS/Linux host")
    tools = {name: find_tool(name, os.environ.get(env)) for name, env in (
        ("clang-cl", "CLANG_CL"), ("lld-link", "LLD_LINK"),
        ("llvm-lib", "LLVM_LIB"), ("llvm-readobj", "LLVM_READOBJ"),
        ("cc", "HOST_CC"))}
    sdk, config = prepare_sdk()
    spec = dependencies()["luajit"]
    revision = revision or spec["revision"]
    repo = repository("luajit", spec, revision)
    parent = ROOT / "build" / "windows-x64"
    parent.mkdir(parents=True, exist_ok=True)
    destination = parent / revision
    print(f"Building Windows x64 from official LuaJIT {revision}", flush=True)
    with tempfile.TemporaryDirectory(prefix="work-", dir=parent) as temporary:
        work = Path(temporary)
        tree, dist = work / "source", work / "dist"
        timestamp = snapshot(repo, revision, tree)
        src = tree / "src"
        dist.mkdir()
        # Keep diagnostics from failed builds as well as successful ones.
        log_path = parent / f"{revision}.log"
        with log_path.open("w") as log:
            def execute(*args):
                log.write("COMMAND: " + repr([str(a) for a in args]) + "\n")
                log.flush()
                try:
                    run(*args, cwd=src, stdout=log, stderr=subprocess.STDOUT)
                except subprocess.CalledProcessError as error:
                    raise RuntimeError(f"Build failed; see {log_path}") from error

            print("Generating Windows VM with native host tools...", flush=True)
            execute(tools["cc"], "-O2", "host/minilua.c", "-lm", "-o", "host/minilua")
            (src / "luajit_relver.txt").write_text(timestamp + "\n")
            execute(src / "host/minilua", "host/genversion.lua")
            execute(src / "host/minilua", "../dynasm/dynasm.lua", "-LN",
                    "-D", "WIN", "-D", "JIT", "-D", "FFI", "-D", "ENDIAN_LE",
                    "-D", "FPU", "-D", "P64", "-o", "host/buildvm_arch.h", "vm_x64.dasc")
            execute(tools["cc"], "-O2", "-I.", "-I../dynasm",
                    "-DLUAJIT_TARGET=LUAJIT_ARCH_X64", "-DLUAJIT_OS=LUAJIT_OS_WINDOWS",
                    *sorted(src.glob("host/buildvm*.c")), "-lm", "-o", "host/buildvm")
            builder = src / "host/buildvm"
            execute(builder, "-m", "peobj", "-o", "lj_vm.obj")
            libraries = library_sources(src)
            for mode in ("bcdef", "ffdef", "libdef", "recdef", "vmdef"):
                filename = "jit/vmdef.lua" if mode == "vmdef" else f"lj_{mode}.h"
                execute(builder, "-m", mode, "-o", filename, *libraries)
            execute(builder, "-m", "folddef", "-o", "lj_folddef.h", "lj_opt_fold.c")

            print("Compiling and linking Windows EXE, DLL and static library...", flush=True)
            includes = [sdk / p for p in ("crt/include", "sdk/include/ucrt", "sdk/include/um", "sdk/include/shared")]
            cflags = ["--target=x86_64-pc-windows-msvc", "/nologo", "/c", "/O2", "/W3", "/MT", "/Z7",
                      "/D_CRT_SECURE_NO_DEPRECATE",
                      "/I.", *[f"/imsvc{p}" for p in includes]]
            libpaths = [sdk / p for p in ("crt/lib/x86_64", "sdk/lib/ucrt/x86_64", "sdk/lib/um/x86_64")]
            ldflags = ["/nologo", "/machine:x64", "/opt:ref", "/opt:icf", "/incremental:no", "/debug", "/ignore:4099",
                       *[f"/libpath:{p}" for p in libpaths]]
            # Follow upstream's CRT inline definition for the engine only.
            engine_flags = ["/D_CRT_STDIO_INLINE=__declspec(dllexport)__inline"]
            execute(tools["clang-cl"], *cflags, *engine_flags, "/DLUA_BUILD_AS_DLL", "ljamalg.c", "/Foljamalg-dll.obj")
            execute(tools["lld-link"], *ldflags, "/dll", f"/out:{dist / 'lua51.dll'}",
                    f"/implib:{dist / 'lua51.lib'}", f"/pdb:{dist / 'lua51.pdb'}",
                    "ljamalg-dll.obj", "lj_vm.obj", "kernel32.lib")
            execute(tools["clang-cl"], *cflags, "/DLUA_BUILD_AS_DLL", "luajit.c", "/Foluajit.obj")
            execute(tools["lld-link"], *ldflags, f"/out:{dist / 'luajit.exe'}", f"/pdb:{dist / 'luajit.pdb'}",
                    "luajit.obj", dist / "lua51.lib", "kernel32.lib")
            execute(tools["clang-cl"], *cflags, *engine_flags, "ljamalg.c", "/Foljamalg-static.obj")
            execute(tools["llvm-lib"], "/nologo", f"/out:{dist / 'lua51-static.lib'}", "ljamalg-static.obj", "lj_vm.obj")
            execute(tools["clang-cl"], *cflags, "/Foembed.obj", "--", ROOT / "tests" / "windows_embed.c")
            execute(tools["lld-link"], *ldflags, f"/out:{dist / 'embed-smoke.exe'}", f"/pdb:{dist / 'embed-smoke.pdb'}",
                    "embed.obj", dist / "lua51-static.lib", "kernel32.lib")

        for name, dll in (("luajit.exe", False), ("lua51.dll", True), ("embed-smoke.exe", False)):
            check_pe(dist / name, dll)
        inspection = output(tools["llvm-readobj"], "--file-headers", "--coff-imports", "--coff-exports",
                            dist / "lua51.dll", dist / "luajit.exe", dist / "embed-smoke.exe")
        if "Name: luaL_newstate" not in inspection or "Name: luaJIT_setmode" not in inspection:
            raise RuntimeError("Required LuaJIT DLL exports missing")
        (dist / "pe-inspection.txt").write_text(inspection)
        shutil.copytree(src / "jit", dist / "lua" / "jit")
        (dist / "include").mkdir()
        for name in ("lua.h", "luaconf.h", "lauxlib.h", "lualib.h", "luajit.h", "lua.hpp"):
            shutil.copy2(src / name, dist / "include" / name)
        shutil.copy2(tree / "COPYRIGHT", dist / "COPYRIGHT-LuaJIT")
        shutil.copy2(ROOT / "tests" / "windows_smoke.lua", dist / "smoke.lua")
        shutil.copy2(log_path, dist / "build.log")
        metadata = {
            "luajit": {"url": spec["url"], "revision": revision, "release_timestamp": timestamp},
            "target": "x86_64-pc-windows-msvc", "crt_linkage": "static (/MT)",
            "toolchain": config,
            "tools": {name: output(path, "--version").splitlines()[0] for name, path in tools.items() if name != "llvm-lib"},
            "upstream_build_recipe_sha256": hashlib.sha256((src / "msvcbuild.bat").read_bytes()).hexdigest(),
            "verification": {"pe_x64": True, "dll_exports": True, "windows_runtime_test": "not run by cross-builder"},
            "sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in dist.iterdir() if p.suffix in (".exe", ".dll", ".lib")},
        }
        write_json(dist / "build-info.json", metadata)
        publish_tree(dist, destination)
    archive = shutil.make_archive(str(parent / f"luajit-windows-x64-{revision}"), "zip", destination)
    print(f"Windows artifacts: {destination}\nZIP: {archive}\nRuntime validation: run luajit.exe smoke.lua and embed-smoke.exe on Windows.")
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--revision", help="Build an explicit full commit SHA without changing dependencies.json")
    parser.add_argument("--prepare-sdk", action="store_true", help="Only download the pinned xwin SDK/CRT")
    args = parser.parse_args()
    if args.prepare_sdk:
        print(prepare_sdk()[0])
    else:
        build_windows(args.revision)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(f"windows-build: {error}")
