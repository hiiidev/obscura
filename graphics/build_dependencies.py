#!/usr/bin/env python3
"""Build a pinned native ANGLE bundle. Run later on each validation platform.

The work directory is owned by this recipe; the maintained engine checkout is
never modified. No prebuilt library is downloaded, and an existing output
bundle is never overwritten. Linux bundles include SwiftShader automatically.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess

HERE = Path(__file__).resolve().parent
PINS = json.loads((HERE / "dependencies.lock.json").read_text())

def run(argv, *, cwd, env, capture=False):
    result = subprocess.run([str(arg) for arg in argv], cwd=cwd, env=env,
                            check=True, text=True, capture_output=capture)
    return result.stdout.strip() if capture else None

def checkout(directory, pin, env):
    if not directory.exists():
        directory.mkdir()
        run(["git", "init", "."], cwd=directory, env=env)
        run(["git", "remote", "add", "origin", pin["repository"]], cwd=directory, env=env)
    else:
        remote = run(["git", "remote", "get-url", "origin"], cwd=directory, env=env, capture=True)
        if remote != pin["repository"]:
            raise RuntimeError(f"Unexpected remote in {directory}")
        if run(["git", "status", "--porcelain", "--untracked-files=no"], cwd=directory, env=env, capture=True):
            raise RuntimeError(f"Refusing to overwrite changes in {directory}")
        if run(["git", "rev-parse", "HEAD"], cwd=directory, env=env, capture=True) == pin["commit"]:
            return
    run(["git", "fetch", "--depth", "1", "origin", pin["commit"]], cwd=directory, env=env)
    run(["git", "checkout", "--detach", pin["commit"]], cwd=directory, env=env)

def bootstrap_depot_tools(depot, env):
    # Auto-update is disabled to retain the pin, so initialize its tools
    # explicitly with the script that does not update the checkout itself.
    run([depot / "ensure_bootstrap"], cwd=depot, env=env)
    if run(["git", "rev-parse", "HEAD"], cwd=depot, env=env, capture=True) != PINS["depot_tools"]["commit"]:
        raise RuntimeError("depot_tools bootstrap changed the pinned revision")
    if run(["git", "status", "--porcelain", "--untracked-files=no"], cwd=depot, env=env, capture=True):
        raise RuntimeError("depot_tools bootstrap changed tracked files")
    # ensure_bootstrap can finish after a failed background setup; verify the
    # same Python wrapper required by gn before expensive dependency work.
    run([depot / "python-bin/python3", "--version"], cwd=depot, env=env)

def gn_arguments(system):
    args = ["is_debug=false", "is_component_build=false", "angle_build_all=false",
            "angle_enable_null=false", "angle_enable_gl=false", "is_clang=true",
            "angle_enable_vulkan_validation_layers=false", "angle_enable_vulkan_api_dump_layer=false"]
    if system == "linux":
        args += ["angle_enable_vulkan=true", "angle_enable_swiftshader=true",
                 "angle_enable_metal=false", "angle_use_x11=false", "angle_use_wayland=false",
                 "angle_use_gbm=false", "angle_use_vulkan_display=true", 'angle_vulkan_display_mode="offscreen"']
    elif system == "macos":
        args += ["angle_enable_metal=true", "angle_enable_vulkan=false", "angle_enable_swiftshader=false"]
    else:
        raise ValueError("Only Linux and macOS dependency bundles are supported")
    return args

def artifact(directory, name):
    direct = directory / name
    if direct.is_file():
        return direct
    candidates = [p for p in directory.rglob(name) if "with_capture" not in p.parts]
    if len(candidates) != 1:
        raise RuntimeError(f"Expected one build output for {name}, found {candidates}")
    return candidates[0]

def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()

def prepare_sysroot(angle, depot, system, machine, env):
    if system != "linux":
        return None
    arch = {"aarch64": "arm64", "arm64": "arm64", "x86_64": "amd64", "AMD64": "amd64"}.get(machine, machine)
    installer = angle / "build/linux/sysroot_scripts/install-sysroot.py"
    metadata = installer.with_name("sysroots.json")
    platform_name = "trixie" if arch == "riscv64" else "bullseye"
    pinned = json.loads(metadata.read_text())[f"{platform_name}_{arch}"]
    # The pinned upstream installer verifies the metadata's SHA-256 before
    # extraction. ANGLE's hooks do not install every native Linux sysroot.
    run([depot / "python-bin/python3", installer, "--arch", arch], cwd=angle, env=env)
    return {"arch": arch, "tarball_sha256": pinned["Sha256Sum"],
            "metadata_sha256": sha(metadata), "installer_sha256": sha(installer)}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work-dir", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--jobs", type=int, default=2)
    args = parser.parse_args()
    system = {"Linux": "linux", "Darwin": "macos"}.get(platform.system())
    if not system or args.jobs < 1:
        parser.error("Requires Linux/macOS and a positive --jobs count")
    work, output = args.work_dir.resolve(), args.output.resolve()
    if output.exists():
        parser.error("--output must not exist; existing bundles are immutable")
    if output.is_relative_to(work) or work.is_relative_to(output):
        parser.error("--output and --work-dir must be separate directory trees")
    owner = work / ".obscura-graphics-workspace.json"
    if work.exists() and any(work.iterdir()) and not owner.is_file():
        parser.error("--work-dir must be empty or owned by this recipe")
    work.mkdir(parents=True, exist_ok=True)
    if owner.exists() and json.loads(owner.read_text()) != PINS:
        parser.error("This workspace uses other pins; choose a new work directory")
    owner.write_text(json.dumps(PINS, indent=2) + "\n")
    env = {**os.environ, "DEPOT_TOOLS_UPDATE": "0", "DEPOT_TOOLS_METRICS": "0"}
    depot = work / "depot_tools"
    checkout(depot, PINS["depot_tools"], env)
    env["PATH"] = str(depot) + os.pathsep + env["PATH"]
    bootstrap_depot_tools(depot, env)
    checkout(work / "angle", PINS["angle"], env)
    config = 'solutions = ' + repr([{"name": "angle", "url": PINS["angle"]["repository"],
                                    "managed": False, "custom_deps": {}, "custom_vars": {}}]) + '\n'
    (work / ".gclient").write_text(config)
    run([depot / "gclient", "sync", "--no-history", "--revision", "angle@" + PINS["angle"]["commit"]], cwd=work, env=env)
    angle = work / "angle"
    actual_angle = run(["git", "rev-parse", "HEAD"], cwd=angle, env=env, capture=True)
    actual_swift = run(["git", "rev-parse", "HEAD"], cwd=angle / "third_party/SwiftShader", env=env, capture=True)
    if actual_angle != PINS["angle"]["commit"] or actual_swift != PINS["swiftshader"]["commit"]:
        raise RuntimeError("gclient dependency revisions differ from the graphics lock")
    sysroot = prepare_sysroot(angle, depot, system, platform.machine(), env)
    build = angle / "out/ObscuraWebGL"
    gn_args = gn_arguments(system)
    run([depot / "gn", "gen", build, "--fail-on-unused-args", "--args=" + " ".join(gn_args)], cwd=angle, env=env)
    targets = ["libEGL", "libGLESv2"] + (["swiftshader"] if system == "linux" else [])
    run([depot / "autoninja", "-C", build, "-j", args.jobs, *targets], cwd=angle, env=env)
    suffix = "dylib" if system == "macos" else "so"
    names = [f"libEGL.{suffix}", f"libGLESv2.{suffix}"]
    if system == "linux":
        # This pinned ANGLE uses a module-local Vulkan loader, including for
        # SwiftShader. Its built loader must travel with EGL/GLES and the ICD.
        names += ["libvulkan.so.1", "libvk_swiftshader.so", "vk_swiftshader_icd.json"]
    sources = {name: artifact(build, name) for name in names}
    # Verify all inputs first; never publish a manifest for a partial bundle.
    staging = output.with_name(output.name + ".building")
    staging.mkdir(parents=True, exist_ok=False)
    try:
        for name, source in sources.items():
            shutil.copy2(source, staging / name)
        if system == "linux":
            path = staging / "vk_swiftshader_icd.json"
            icd = json.loads(path.read_text())
            icd["ICD"]["library_path"] = "./libvk_swiftshader.so"
            path.write_text(json.dumps(icd, indent=2) + "\n")
        # Preserve upstream licenses and third-party notices alongside binaries.
        licenses = staging / "licenses"
        for folder, children, files in os.walk(angle):
            children[:] = [c for c in children if c not in {".git", "out", "node_modules", "__pycache__"}]
            for name in files:
                if name.upper().startswith(("LICENSE", "COPYING", "NOTICE")):
                    source = Path(folder) / name
                    if source.is_symlink() or not source.is_file():
                        continue
                    dest = licenses / source.relative_to(angle)
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(source, dest)
        (staging / "dependencies.lock.json").write_text(json.dumps(PINS, indent=2) + "\n")
        arch = {"arm64": "aarch64", "AMD64": "x86_64"}.get(platform.machine(), platform.machine())
        manifest = {"schema": 1, "os": system, "arch": arch,
                    "angle_commit": actual_angle, "swiftshader_commit": actual_swift,
                    "depot_tools_commit": PINS["depot_tools"]["commit"], "gn_args": gn_args,
                    "sysroot": sysroot,
                    "host": platform.platform(), "files": {name: sha(staging / name) for name in names},
                    "validation": "pending: bundle creation does not establish rendering or conformance"}
        (staging / "bundle.json").write_text(json.dumps(manifest, indent=2) + "\n")
        staging.rename(output)
    except BaseException:
        # Retain failed build evidence for diagnosis, without a final bundle.
        raise
    print(json.dumps({"bundle": str(output), "manifest_sha256": sha(output / "bundle.json")}))

if __name__ == "__main__":
    main()
