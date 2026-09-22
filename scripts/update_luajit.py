#!/usr/bin/env python3
"""Check official LuaJIT v2.1; validate a candidate before changing the pinned SHA."""
import argparse
import json
import os
import subprocess
import sys

from bootstrap import build_native
from build_common import ROOT, dependencies, output, run, validate_revision, write_json
from build_windows import build_windows
from test_binder import native as test_native_binder, windows as build_windows_binder


def latest_revision(spec):
    ref = "refs/heads/" + spec.get("branch", "v2.1")
    lines = output("git", "ls-remote", "--exit-code", spec["url"], ref).splitlines()
    if len(lines) != 1 or lines[0].split()[1] != ref:
        raise RuntimeError(f"Could not resolve official branch {ref}")
    return validate_revision(lines[0].split()[0])


def validate_candidate(revision):
    runtime = build_native(revision)
    build_windows(revision)
    test_native_binder(revision, sanitize=False)
    build_windows_binder(revision)
    env = os.environ.copy()
    env["TYNEX_LUAJIT"] = str(runtime)
    run("cargo", "test", "--workspace", "--locked", cwd=ROOT, env=env)
    run(sys.executable, "-m", "unittest", "discover", "-s", "tests", "-v", cwd=ROOT, env=env)


def apply_update(revision):
    validate_revision(revision)
    path = ROOT / "dependencies.json"
    original = path.read_bytes()
    spec = json.loads(original)
    previous = spec["luajit"]["revision"]
    validate_candidate(revision)
    if path.read_bytes() != original:
        raise RuntimeError("dependencies.json changed during validation; leaving it untouched")
    spec["luajit"]["revision"] = revision
    write_json(path, spec)
    print(f"Pinned LuaJIT: {previous} -> {revision}")
    print("Native tests and Windows cross-build passed. Windows runtime tests remain a separate CI/device gate.")
    print(f"Rollback: python3 scripts/update_luajit.py --apply --revision {previous}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="Only report the candidate (default)")
    mode.add_argument("--apply", action="store_true", help="Build/test the candidate, then atomically pin it")
    mode.add_argument("--verify", action="store_true", help="Rebuild/test the current pin without changing it")
    parser.add_argument("--revision", help="Explicit full commit SHA, also usable for rollback")
    args = parser.parse_args()
    spec = dependencies()["luajit"]
    current = spec["revision"]
    candidate = validate_revision(args.revision) if args.revision else (current if args.verify else latest_revision(spec))
    print(f"Current:   {current}\nCandidate: {candidate}", flush=True)
    if args.verify:
        validate_candidate(candidate)
    elif args.apply and candidate != current:
        apply_update(candidate)
    elif candidate == current:
        print("Already pinned to this commit. Use --verify to rebuild and validate it.")
    else:
        print("No files changed. Use --apply to validate and adopt this commit.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(f"update-luajit: {error}")
