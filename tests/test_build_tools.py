"""Safety regressions for upstream updates and isolated build outputs."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import build_common
import build_windows
import update_luajit


class SourceSnapshotTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / ".deps" / "luajit"
        self.repo.mkdir(parents=True)
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "user.name", "Build Test")
        self.git("remote", "add", "origin", "https://github.com/LuaJIT/LuaJIT.git")
        (self.repo / "source.c").write_text("first version\n")
        (self.repo / ".gitignore").write_text("generated.h\n")
        self.git("add", ".")
        self.git("commit", "-qm", "first")
        self.first = self.git("rev-parse", "HEAD")
        (self.repo / "source.c").write_text("second version\n")
        self.git("commit", "-qam", "second")
        self.second = self.git("rev-parse", "HEAD")
        self.spec = {"url": "https://github.com/LuaJIT/LuaJIT.git", "revision": self.first}

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args], text=True).strip()

    def test_other_revision_is_archived_without_switching_or_copying_generated_files(self):
        (self.repo / "generated.h").write_text("stale architecture header")
        with patch.object(build_common, "ROOT", self.root):
            repo = build_common.repository("luajit", self.spec)
            build_common.snapshot(repo, self.first, self.root / "snapshot")
        self.assertEqual(self.git("rev-parse", "HEAD"), self.second)
        self.assertEqual((self.root / "snapshot/source.c").read_text(), "first version\n")
        self.assertFalse((self.root / "snapshot/generated.h").exists())
        self.assertEqual((self.repo / "generated.h").read_text(), "stale architecture header")
        self.assertTrue((self.root / "snapshot/.relver").read_text().strip().isdigit())

    def test_local_edits_are_preserved_and_rejected(self):
        (self.repo / "source.c").write_text("user edits\n")
        with patch.object(build_common, "ROOT", self.root):
            with self.assertRaisesRegex(RuntimeError, "local changes"):
                build_common.repository("luajit", self.spec)
        self.assertEqual((self.repo / "source.c").read_text(), "user edits\n")

    def test_changed_origin_is_rejected(self):
        self.git("remote", "set-url", "origin", "https://example.invalid/fork.git")
        with patch.object(build_common, "ROOT", self.root):
            with self.assertRaisesRegex(RuntimeError, "origin differs"):
                build_common.repository("luajit", self.spec)

    def test_branch_names_cannot_replace_pinned_commit_ids(self):
        for revision in ("main", "--help", "../outside", self.first[:7]):
            with self.subTest(revision=revision), self.assertRaises(ValueError):
                build_common.validate_revision(revision)


class UpdateTransactionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.path = self.root / "dependencies.json"
        self.old, self.new = "a" * 40, "b" * 40
        self.spec = {"luajit": {"revision": self.old, "branch": "v2.1"}, "metadata": {"version": "unchanged"}}
        self.path.write_text(json.dumps(self.spec))

    def test_failed_validation_does_not_change_pin(self):
        original = self.path.read_bytes()
        with patch.object(update_luajit, "ROOT", self.root), \
             patch.object(update_luajit, "validate_candidate", side_effect=RuntimeError("build failed")):
            with self.assertRaisesRegex(RuntimeError, "build failed"):
                update_luajit.apply_update(self.new)
        self.assertEqual(self.path.read_bytes(), original)

    def test_binder_failure_blocks_candidate_update_before_pin_changes(self):
        original = self.path.read_bytes()
        with patch.object(update_luajit, "ROOT", self.root), \
             patch.object(update_luajit, "build_native", return_value=self.root / "luajit"), \
             patch.object(update_luajit, "build_windows"), \
             patch.object(update_luajit, "build_native_task"), \
             patch.object(update_luajit, "build_windows_task"), \
             patch.object(update_luajit, "test_native_binder") as native, \
             patch.object(update_luajit, "build_windows_binder", side_effect=RuntimeError("binder failed")) as windows, \
             patch.object(update_luajit, "run") as run:
            with self.assertRaisesRegex(RuntimeError, "binder failed"):
                update_luajit.apply_update(self.new)
            native.assert_called_once_with(self.new, sanitize=False)
            windows.assert_called_once_with(self.new)
            run.assert_not_called()
        self.assertEqual(self.path.read_bytes(), original)

    def test_task_failure_blocks_candidate_update(self):
        original = self.path.read_bytes()
        with patch.object(update_luajit, "ROOT", self.root), \
             patch.object(update_luajit, "build_native", return_value=self.root / "luajit"), \
             patch.object(update_luajit, "build_windows"), \
             patch.object(update_luajit, "build_native_task"), \
             patch.object(update_luajit, "build_windows_task", side_effect=RuntimeError("task module failed")), \
             patch.object(update_luajit, "test_native_binder") as binder, \
             patch.object(update_luajit, "run") as run:
            with self.assertRaisesRegex(RuntimeError, "task module failed"):
                update_luajit.apply_update(self.new)
            binder.assert_not_called()
            run.assert_not_called()
        self.assertEqual(self.path.read_bytes(), original)

    def test_pin_changes_only_after_validation_and_preserves_other_dependencies(self):
        def validate(revision):
            self.assertEqual(revision, self.new)
            self.assertEqual(json.loads(self.path.read_text()), self.spec)
        with patch.object(update_luajit, "ROOT", self.root), \
             patch.object(update_luajit, "validate_candidate", side_effect=validate):
            update_luajit.apply_update(self.new)
        actual = json.loads(self.path.read_text())
        self.assertEqual(actual["luajit"]["revision"], self.new)
        self.assertEqual(actual["metadata"], self.spec["metadata"])

    def test_concurrent_dependency_edit_is_not_overwritten(self):
        def edit(_):
            self.path.write_text('{"user": "edit"}')
        with patch.object(update_luajit, "ROOT", self.root), \
             patch.object(update_luajit, "validate_candidate", side_effect=edit):
            with self.assertRaisesRegex(RuntimeError, "changed during validation"):
                update_luajit.apply_update(self.new)
        self.assertEqual(json.loads(self.path.read_text()), {"user": "edit"})


class BuildOutputTests(unittest.TestCase):
    def test_managed_output_is_replaced_across_project_rename(self):
        for marker in (".tljit-build", ".tynex-build"):
            with self.subTest(marker=marker), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                source, dest = root / "source", root / "destination"
                source.mkdir()
                dest.mkdir()
                (dest / marker).write_text("Generated by scripts/build_common.py\n")
                (dest / "old.txt").write_text("old build")
                (source / "new.txt").write_text("new build")
                build_common.publish_tree(source, dest)
                self.assertEqual((dest / "new.txt").read_text(), "new build")
                self.assertTrue((dest / ".tynex-build").is_file())
                self.assertFalse((dest / "old.txt").exists())
                self.assertFalse((dest / ".tljit-build").exists())

    def test_matching_legacy_sdk_is_reused_without_download(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = {"arch": "x86_64", "sdk_version": "test-sdk", "crt_version": "test-crt",
                      "xwin_version": "test-xwin", "variant": "desktop"}
            (root / "windows-toolchain.json").write_text(json.dumps(config))
            with patch.object(build_windows, "ROOT", root):
                sdk = build_windows.sdk_root(config)
                for name in ("crt/include", "sdk/include/ucrt", "sdk/include/um", "sdk/include/shared",
                             "crt/lib/x86_64", "sdk/lib/ucrt/x86_64", "sdk/lib/um/x86_64"):
                    (sdk / name).mkdir(parents=True)
                (sdk / ".tljit-sdk.json").write_text(json.dumps(config))
                with patch.object(build_windows, "find_tool", return_value="xwin"), \
                     patch.object(build_windows, "output", return_value="xwin test-xwin"), \
                     patch.object(build_windows, "run") as run:
                    self.assertEqual(build_windows.prepare_sdk(), (sdk, config))
                    run.assert_not_called()
                self.assertEqual(json.loads((sdk / ".tynex-sdk.json").read_text()), config)

    def test_unmanaged_output_is_never_replaced(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, dest = root / "source", root / "destination"
            source.mkdir()
            dest.mkdir()
            (dest / "valuable.txt").write_text("keep")
            with self.assertRaisesRegex(RuntimeError, "unmanaged"):
                build_common.publish_tree(source, dest)
            self.assertEqual((dest / "valuable.txt").read_text(), "keep")

    def test_upstream_library_addition_is_picked_up_in_order(self):
        with tempfile.TemporaryDirectory() as directory:
            src = Path(directory)
            (src / "msvcbuild.bat").write_text("@set ALL_LIB=lib_base.c lib_future.c\n")
            for name in ("lib_base.c", "lib_future.c"):
                (src / name).touch()
            self.assertEqual(build_windows.library_sources(src), ["lib_base.c", "lib_future.c"])
            (src / "msvcbuild.bat").write_text("@set NEW_FORMAT=something\n")
            with self.assertRaisesRegex(RuntimeError, "review"):
                build_windows.library_sources(src)

    def test_non_pe_artifact_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "luajit.exe"
            path.write_bytes(b"not actually a Windows executable")
            with self.assertRaisesRegex(RuntimeError, "Not a Windows PE"):
                build_windows.check_pe(path)
