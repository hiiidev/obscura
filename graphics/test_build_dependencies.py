"""Offline recipe tests. Mocks never fetch, build or load native graphics.

Authoring these tests is separate from running them during platform validation.
Run with: python3 -m unittest discover -s graphics -p test_build_dependencies.py
"""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("graphics_recipe", Path(__file__).with_name("build_dependencies.py"))
recipe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recipe)


class DependencyRecipeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        # Match the production CLI's canonical work/output paths on macOS too.
        self.root = Path(self.temp.name).resolve()
        self.work, self.output = self.root / "work", self.root / "bundle"

    def test_gn_backends_have_explicit_headless_and_platform_gates(self):
        linux = recipe.gn_arguments("linux")
        self.assertIn("angle_enable_vulkan=true", linux)
        self.assertIn("angle_enable_swiftshader=true", linux)
        for key in ["x11", "wayland", "gbm"]:
            self.assertIn(f"angle_use_{key}=false", linux)
        self.assertIn('angle_vulkan_display_mode="offscreen"', linux)
        mac = recipe.gn_arguments("macos")
        self.assertIn("angle_enable_metal=true", mac)
        self.assertIn("angle_enable_swiftshader=false", mac)
        self.assertIn("angle_enable_vulkan=false", mac)
        with self.assertRaises(ValueError):
            recipe.gn_arguments("unsupported")

    def test_checkout_creates_only_the_requested_pin(self):
        path = self.root / "angle"
        pin = recipe.PINS["angle"]
        with patch.object(recipe, "run") as run:
            recipe.checkout(path, pin, {})
        self.assertTrue(path.is_dir())
        commands = [call.args[0] for call in run.call_args_list]
        self.assertEqual(commands, [["git", "init", "."],
            ["git", "remote", "add", "origin", pin["repository"]],
            ["git", "fetch", "--depth", "1", "origin", pin["commit"]],
            ["git", "checkout", "--detach", pin["commit"]]])

    def test_checkout_refuses_foreign_remote_or_tracked_modifications(self):
        path = self.root / "angle"; path.mkdir(); pin = recipe.PINS["angle"]
        for replies in [["https://unexpected.example/repo"], [pin["repository"], " M source.cc"]]:
            with self.subTest(replies=replies), patch.object(recipe, "run", side_effect=replies) as run:
                with self.assertRaises(RuntimeError): recipe.checkout(path, pin, {})
                self.assertFalse(any(call.args[0][1] in ["fetch", "checkout"] for call in run.call_args_list))

    def test_checkout_reuses_exact_clean_pin_and_fetches_a_different_clean_pin(self):
        path = self.root / "angle"; path.mkdir(); pin = recipe.PINS["angle"]
        with patch.object(recipe, "run", side_effect=[pin["repository"], "", pin["commit"]]) as run:
            recipe.checkout(path, pin, {})
            self.assertEqual(run.call_count, 3)
        with patch.object(recipe, "run", side_effect=[pin["repository"], "", "older", None, None]) as run:
            recipe.checkout(path, pin, {})
            self.assertEqual(run.call_args_list[-1].args[0], ["git", "checkout", "--detach", pin["commit"]])

    def test_process_errors_propagate_and_capture_is_explicit(self):
        result = subprocess.CompletedProcess(["tool"], 0, " result\n", "")
        with patch.object(recipe.subprocess, "run", return_value=result) as run:
            self.assertEqual(recipe.run([Path("tool")], cwd=self.root, env={}, capture=True), "result")
            self.assertTrue(run.call_args.kwargs["check"])
            self.assertIsNone(recipe.run(["tool"], cwd=self.root, env={}))
        with patch.object(recipe.subprocess, "run", side_effect=subprocess.CalledProcessError(1, ["tool"])):
            with self.assertRaises(subprocess.CalledProcessError): recipe.run(["tool"], cwd=self.root, env={})

    def test_depot_bootstrap_retains_the_pin_and_checks_the_python_wrapper(self):
        depot = self.work / "depot_tools"
        env = {"DEPOT_TOOLS_UPDATE": "0"}
        with patch.object(recipe, "run", side_effect=[None, recipe.PINS["depot_tools"]["commit"], "", None]) as run:
            recipe.bootstrap_depot_tools(depot, env)
        self.assertEqual([call.args[0] for call in run.call_args_list], [
            [depot / "ensure_bootstrap"], ["git", "rev-parse", "HEAD"],
            ["git", "status", "--porcelain", "--untracked-files=no"],
            [depot / "python-bin/python3", "--version"]])
        self.assertTrue(all(call.kwargs["cwd"] == depot and call.kwargs["env"] == env
                            for call in run.call_args_list))

    def test_depot_bootstrap_failures_and_pin_drift_stop_before_building(self):
        pin = recipe.PINS["depot_tools"]["commit"]
        failures = [([subprocess.CalledProcessError(1, ["ensure_bootstrap"])], subprocess.CalledProcessError),
                    ([None, "other"], RuntimeError), ([None, pin, " M gn.py"], RuntimeError),
                    ([None, pin, "", subprocess.CalledProcessError(1, ["python3"])], subprocess.CalledProcessError)]
        for replies, expected in failures:
            with self.subTest(replies=replies), patch.object(recipe, "run", side_effect=replies) as run:
                with self.assertRaises(expected): recipe.bootstrap_depot_tools(self.work / "depot_tools", {})
                self.assertEqual(run.call_count, len(replies))
        with patch.object(recipe, "bootstrap_depot_tools", side_effect=RuntimeError("bootstrap failed")):
            with self.assertRaisesRegex(RuntimeError, "bootstrap failed"): self.invoke()
        self.assertFalse((self.work / "angle").exists())
        self.assertFalse(self.output.exists())

    def test_artifact_selection_rejects_missing_and_ambiguous_results(self):
        build = self.root / "build"; build.mkdir()
        with self.assertRaises(RuntimeError): recipe.artifact(build, "libEGL.so")
        for part in ["one", "two", "with_capture"]:
            folder = build / part; folder.mkdir(); (folder / "libEGL.so").write_bytes(part.encode())
        with self.assertRaises(RuntimeError): recipe.artifact(build, "libEGL.so")
        (build / "two/libEGL.so").unlink()
        self.assertEqual(recipe.artifact(build, "libEGL.so"), build / "one/libEGL.so")
        (build / "libEGL.so").write_bytes(b"direct")
        self.assertEqual(recipe.artifact(build, "libEGL.so"), build / "libEGL.so")

    def invoke(self, system="Linux", extra=(), wrong_revision=False, build_error=False, copy_error=False,
               machine="arm64", sysroot_error=False, missing_loader=False):
        argv = ["build_dependencies.py", "--work-dir", str(self.work), "--output", str(self.output), *extra]
        def checkout(directory, pin, env):
            directory.mkdir(parents=True, exist_ok=True)
            if directory.name == "angle":
                (directory / "third_party/SwiftShader").mkdir(parents=True)
                (directory / "LICENSE").write_text("upstream license\n")
                (directory / "NOTICE-link").symlink_to(directory / "LICENSE")
                installer = directory / "build/linux/sysroot_scripts/install-sysroot.py"
                installer.parent.mkdir(parents=True)
                installer.write_text("# Mocked pinned installer; never executed by these tests.\n")
                installer.with_name("sysroots.json").write_text(json.dumps({
                    "bullseye_arm64": {"Sha256Sum": "a" * 64}, "bullseye_amd64": {"Sha256Sum": "b" * 64}}))
        def run(argv, *, cwd, env, capture=False):
            command = [str(a) for a in argv]
            if command[:2] == ["git", "rev-parse"]:
                key = {"SwiftShader": "swiftshader", "depot_tools": "depot_tools"}.get(Path(cwd).name, "angle")
                return "wrong" if wrong_revision and key != "depot_tools" else recipe.PINS[key]["commit"]
            if len(command) > 1 and Path(command[1]).name == "install-sysroot.py" and sysroot_error:
                raise subprocess.CalledProcessError(1, command)
            if Path(command[0]).name == "autoninja":
                if build_error: raise subprocess.CalledProcessError(1, command)
                build = Path(command[command.index("-C") + 1]); build.mkdir(parents=True)
                suffix = "dylib" if system == "Darwin" else "so"
                for name in [f"libEGL.{suffix}", f"libGLESv2.{suffix}", "libvk_swiftshader.so"]:
                    (build / name).write_bytes(name.encode())
                if not missing_loader:
                    (build / "libvulkan.so.1").write_bytes(b"pinned built Vulkan loader")
                (build / "vk_swiftshader_icd.json").write_text(json.dumps({"ICD": {"library_path": "/temporary/path.so"}}))
            return None
        with patch("sys.argv", argv), patch.object(recipe.platform, "system", return_value=system), \
             patch.object(recipe.platform, "machine", return_value=machine), \
             patch.object(recipe, "checkout", side_effect=checkout), patch.object(recipe, "run", side_effect=run) as commands, \
             patch("builtins.print"):
            if copy_error:
                with patch.object(recipe.shutil, "copy2", side_effect=OSError("injected copy failure")): recipe.main()
            else: recipe.main()
        return [call.args[0] for call in commands.call_args_list]

    def test_linux_bundle_records_verified_inputs_relative_icd_and_licenses(self):
        self.invoke()
        manifest = json.loads((self.output / "bundle.json").read_text())
        self.assertEqual(manifest["os"], "linux"); self.assertEqual(manifest["arch"], "aarch64")
        self.assertEqual(manifest["angle_commit"], recipe.PINS["angle"]["commit"])
        self.assertEqual(manifest["swiftshader_commit"], recipe.PINS["swiftshader"]["commit"])
        self.assertEqual(set(manifest["files"]), {"libEGL.so", "libGLESv2.so", "libvulkan.so.1",
                                                 "libvk_swiftshader.so", "vk_swiftshader_icd.json"})
        self.assertEqual((self.output / "libvulkan.so.1").read_bytes(), b"pinned built Vulkan loader")
        for name, digest in manifest["files"].items():
            self.assertEqual(digest, hashlib.sha256((self.output / name).read_bytes()).hexdigest())
        self.assertEqual(json.loads((self.output / "vk_swiftshader_icd.json").read_text())["ICD"]["library_path"], "./libvk_swiftshader.so")
        self.assertEqual((self.output / "licenses/LICENSE").read_text(), "upstream license\n")
        self.assertFalse((self.output / "licenses/NOTICE-link").exists())
        self.assertFalse(self.output.with_name("bundle.building").exists())
        self.assertIn("pending", manifest["validation"])

    def test_missing_built_vulkan_loader_never_publishes_a_linux_bundle(self):
        with self.assertRaisesRegex(RuntimeError, "libvulkan.so.1"):
            self.invoke(missing_loader=True)
        self.assertFalse(self.output.exists())
        self.assertFalse(self.output.with_name("bundle.building").exists())

    def test_macos_bundle_omits_software_libraries(self):
        commands = self.invoke(system="Darwin", sysroot_error=True)
        manifest = json.loads((self.output / "bundle.json").read_text())
        files = manifest["files"]
        self.assertEqual(set(files), {"libEGL.dylib", "libGLESv2.dylib"})
        self.assertIsNone(manifest["sysroot"])
        self.assertFalse(any("install-sysroot.py" in str(arg) for command in commands for arg in command))

    def test_linux_sysroot_uses_native_architecture_and_pinned_metadata_before_gn(self):
        for machine, arch, checksum in [("aarch64", "arm64", "a" * 64), ("x86_64", "amd64", "b" * 64)]:
            with self.subTest(machine=machine):
                self.work = self.root / f"work-{machine}"; self.output = self.root / f"bundle-{machine}"
                commands = self.invoke(machine=machine)
                installer = self.work / "angle/build/linux/sysroot_scripts/install-sysroot.py"
                install = [self.work / "depot_tools/python-bin/python3", installer, "--arch", arch]
                self.assertIn(install, commands)
                self.assertLess(commands.index(install), next(i for i, command in enumerate(commands) if Path(command[0]).name == "gn"))
                metadata = json.loads((self.output / "bundle.json").read_text())["sysroot"]
                self.assertEqual(metadata, {"arch": arch, "tarball_sha256": checksum,
                    "metadata_sha256": recipe.sha(installer.with_name("sysroots.json")), "installer_sha256": recipe.sha(installer)})

    def test_linux_sysroot_failure_never_publishes_a_bundle(self):
        with self.assertRaises(subprocess.CalledProcessError): self.invoke(sysroot_error=True)
        self.assertFalse(self.output.exists())
        self.assertFalse((self.work / "angle/out").exists())

    def test_output_is_immutable_and_unowned_workspace_is_not_modified(self):
        self.output.mkdir(); sentinel = self.output / "keep"; sentinel.write_text("unchanged")
        with self.assertRaises(SystemExit): self.invoke()
        self.assertEqual(sentinel.read_text(), "unchanged")
        sentinel.unlink(); self.output.rmdir(); self.work.mkdir(); (self.work / "keep").write_text("unchanged")
        with self.assertRaises(SystemExit): self.invoke()
        self.assertEqual((self.work / "keep").read_text(), "unchanged")

    def test_foreign_workspace_pins_and_overlapping_paths_are_rejected(self):
        self.work.mkdir(); owner = self.work / ".obscura-graphics-workspace.json"; owner.write_text('{}')
        with self.assertRaises(SystemExit): self.invoke()
        self.assertEqual(owner.read_text(), '{}')
        owner.unlink(); self.work.rmdir()
        self.output = self.work / "nested"
        with self.assertRaises(SystemExit): self.invoke()
        self.output = self.root / "parent"; self.work = self.output / "nested"
        with self.assertRaises(SystemExit): self.invoke()

    def test_invalid_platform_and_job_count_fail_before_checkout(self):
        for system, extra in [("Windows", ()), ("Linux", ("--jobs", "0")), ("Linux", ("--jobs", "-1"))]:
            with self.subTest(system=system, extra=extra), self.assertRaises(SystemExit): self.invoke(system, extra)
        self.assertFalse(self.work.exists())

    def test_dependency_revision_and_build_failures_never_publish_a_bundle(self):
        for flag, expected in [("wrong_revision", RuntimeError), ("build_error", subprocess.CalledProcessError)]:
            # Each injected failure needs its own mocked checkout and build state.
            self.work = self.root / flag
            with self.subTest(flag=flag), self.assertRaises(expected):
                self.invoke(**{flag: True})
            self.assertFalse(self.output.exists())

    def test_copy_failure_retains_staging_evidence_without_publishing(self):
        with self.assertRaises(OSError): self.invoke(copy_error=True)
        self.assertFalse(self.output.exists())
        self.assertTrue(self.output.with_name("bundle.building").is_dir())


if __name__ == "__main__":
    unittest.main()
