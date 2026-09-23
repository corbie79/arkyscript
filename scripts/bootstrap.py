#!/usr/bin/env python3
"""Build a pinned native LuaJIT from an isolated upstream snapshot."""
import os
import platform
import subprocess
import sys
import tempfile
from pathlib import Path

from build_common import ROOT, dependencies, publish_tree, repository, run, snapshot


def build_native(revision=None):
    spec = dependencies()["luajit"]
    revision = revision or spec["revision"]
    repo = repository("luajit", spec, revision)
    parent = ROOT / "build" / "native"
    parent.mkdir(parents=True, exist_ok=True)
    destination = parent / revision
    with tempfile.TemporaryDirectory(prefix="work-", dir=parent) as temporary:
        source = Path(temporary) / "source"
        snapshot(repo, revision, source)
        env = os.environ.copy()
        if platform.system() == "Darwin":
            env.setdefault("MACOSX_DEPLOYMENT_TARGET", ".".join(platform.mac_ver()[0].split(".")[:2]))
        debug = f"CCDEBUG=-g -fdebug-prefix-map={source}={destination}"
        run("make", "-C", source, f"-j{min(os.cpu_count() or 2, 8)}", debug, env=env)
        run(source / "src" / "luajit", "-v")
        publish_tree(source, destination)
    return destination / "src" / "luajit"


def main():
    from build_task import native as build_task

    runtime = build_native()
    build_task(dependencies()["luajit"]["revision"])
    run("cargo", "build", "--workspace", "--locked", cwd=ROOT)
    print(f"Ready: {runtime}\nRun: ./bin/tynex run examples/main.arky")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(f"bootstrap: {error}")
