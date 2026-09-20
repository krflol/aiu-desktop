#!/usr/bin/env python3
"""Build native desktop and standalone Go CLI release archives."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path


CLI_TARGETS = (
    ("windows", "amd64"),
    ("windows", "arm64"),
    ("linux", "amd64"),
    ("linux", "arm64"),
    ("macos", "amd64"),
    ("macos", "arm64"),
)
CLI_CHOICES = {f"{name}-{arch}": (name, arch) for name, arch in CLI_TARGETS}


def run(command: list[str], cwd: Path, env: dict[str, str] | None = None) -> None:
    subprocess.run(command, cwd=cwd, env=env, check=True)


def native_target() -> tuple[str, str]:
    system = platform.system()
    machine = platform.machine().lower()
    if system == "Windows":
        if machine not in {"amd64", "x86_64"}:
            raise SystemExit("Windows packaging currently supports amd64 only")
        return "windows", "amd64"
    if system == "Linux":
        if machine not in {"amd64", "x86_64"}:
            raise SystemExit("Linux packaging currently supports amd64 only")
        return "linux", "amd64"
    if system == "Darwin":
        return "macos", "arm64" if machine in {"arm64", "aarch64"} else "amd64"
    raise SystemExit("supported native desktop hosts: Windows, Linux, macOS")


def read_backend_manifest(root: Path, backend: Path) -> tuple[dict, str]:
    manifest = json.loads((root / "backend.json").read_text(encoding="utf-8"))
    revision = manifest.get("revision", "")
    repository = manifest.get("source_repository", "")
    if not repository or not revision or revision == "PLACEHOLDER":
        raise SystemExit("backend.json must contain a pinned source_repository and revision")
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=backend, text=True).strip()
    if actual != revision:
        raise SystemExit(f"backend revision mismatch: expected {revision}, got {actual}")
    dirty = subprocess.check_output(["git", "status", "--porcelain"], cwd=backend, text=True)
    if dirty.strip():
        raise SystemExit("backend source must have a clean worktree")
    return manifest, revision


def backend_version(backend: Path) -> str:
    text = (backend / "Makefile").read_text(encoding="utf-8")
    match = re.search(r"^VERSION\s*\?=\s*([0-9]+\.[0-9]+\.[0-9]+)\s*$", text, re.MULTILINE)
    if not match:
        raise SystemExit("could not read backend VERSION from Makefile")
    return match.group(1)


def go_env(goos: str, goarch: str) -> dict[str, str]:
    if goos == "darwin" and (platform.system() != "Darwin" or native_target()[1] != goarch):
        raise SystemExit("macOS Go binaries must be built natively with CGO_ENABLED=1")
    return {**os.environ, "CGO_ENABLED": "1" if goos == "darwin" else "0", "GOOS": goos, "GOARCH": goarch}


def build_cli(backend: Path, destination: Path, goos: str, goarch: str, version: str) -> None:
    env = go_env(goos, goarch)
    ext = ".exe" if goos == "windows" else ""
    run(
        ["go", "build", "-trimpath", "-ldflags", f"-s -w -X github.com/getparable/aiu/internal/core.Version={version}", "-o", str(destination), "./cmd/aiu"],
        backend,
        env,
    )


def archive(files: list[Path], output: Path, prefix: str, windows: bool) -> None:
    entries: list[tuple[Path, str]] = []
    for item in files:
        if item.is_dir():
            entries.extend((path, f"{prefix}/{item.name}/{path.relative_to(item).as_posix()}") for path in item.rglob("*"))
        else:
            entries.append((item, f"{prefix}/{item.name}"))
    if windows:
        with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as bundle:
            for item, name in entries:
                bundle.write(item, name)
        return
    with tarfile.open(output, "w:gz") as bundle:
        for item, name in entries:
            info = bundle.gettarinfo(str(item), name)
            if item.is_dir() or item.name in {"aiu", "aiu.exe", "aiu-desktop"}:
                info.mode = 0o755
            else:
                info.mode = 0o644
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            if item.is_file():
                with item.open("rb") as stream:
                    bundle.addfile(info, stream)
            else:
                bundle.addfile(info)


def provenance(path: Path, manifest: dict, revision: str, version: str) -> None:
    path.write_text(
        f"repository: {manifest['source_repository']}\n"
        f"revision: {revision}\n"
        f"backend_version: {version}\n"
        f"contract_version: {manifest.get('contract_version', '')}\n",
        encoding="utf-8",
    )


def package_desktop(root: Path, backend: Path, output: Path, frontend_version: str, manifest: dict, revision: str, backend_ver: str) -> Path:
    label, arch = native_target()
    goos = "darwin" if label == "macos" else label
    ext = ".exe" if goos == "windows" else ""
    with tempfile.TemporaryDirectory(prefix="aiu-desktop-") as temp_name:
        temp = Path(temp_name)
        stage = temp / "bundle"
        stage.mkdir()
        rustbin = root / "target" / "release" / f"aiu-desktop{ext}"
        run(["cargo", "build", "--locked", "--release", "--bin", "aiu-desktop"], root)
        if not rustbin.is_file():
            raise SystemExit(f"missing {rustbin}")
        cli = stage / f"aiu{ext}"
        build_cli(backend, cli, goos, arch, backend_ver)
        shutil.copy2(rustbin, stage / rustbin.name)
        shutil.copyfile(backend / "LICENSE", stage / "LICENSE")
        shutil.copyfile(root / "docs" / "platform-support.md", stage / "PLATFORM.md")
        shutil.copyfile(root / "docs" / "attribution.md", stage / "NOTICE")
        provenance(stage / "BACKEND.txt", manifest, revision, backend_ver)
        if goos == "darwin":
            app = temp / "AIU.app" / "Contents"
            (app / "MacOS").mkdir(parents=True)
            (app / "Resources").mkdir()
            shutil.copy2(rustbin, app / "MacOS" / "aiu-desktop")
            shutil.copy2(cli, app / "MacOS" / "aiu")
            (app / "Info.plist").write_text(
                '<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>'
                '<key>CFBundleDisplayName</key><string>AIU</string>'
                '<key>CFBundleExecutable</key><string>aiu-desktop</string>'
                '<key>CFBundleIdentifier</key><string>dev.aiu.desktop</string>'
                '<key>CFBundlePackageType</key><string>APPL</string>'
                f"<key>CFBundleShortVersionString</key><string>{frontend_version}</string>"
                "</dict></plist>",
                encoding="utf-8",
            )
            run(["codesign", "--force", "--deep", "--sign", "-", str(temp / "AIU.app")], root)
            files = [temp / "AIU.app", stage / "LICENSE", stage / "PLATFORM.md", stage / "NOTICE", stage / "BACKEND.txt"]
            suffix, prefix = ".zip", "aiu-desktop"
        else:
            files = [stage / rustbin.name, cli, stage / "LICENSE", stage / "PLATFORM.md", stage / "NOTICE", stage / "BACKEND.txt"]
            suffix, prefix = (".zip", "aiu-desktop") if goos == "windows" else (".tar.gz", "aiu-desktop")
        output.mkdir(parents=True, exist_ok=True)
        result = output / f"aiu-desktop-{frontend_version}-{label}-{arch}{suffix}"
        archive(files, result, prefix, suffix == ".zip")
        return result


def package_cli(root: Path, backend: Path, output: Path, frontend_version: str, manifest: dict, revision: str, backend_ver: str, targets: list[tuple[str, str]]) -> list[Path]:
    output.mkdir(parents=True, exist_ok=True)
    outputs = []
    for target, arch in targets:
        with tempfile.TemporaryDirectory(prefix="aiu-cli-") as temp_name:
            temp = Path(temp_name)
            ext = ".exe" if target == "windows" else ""
            binary = temp / f"aiu{ext}"
            build_cli(backend, binary, "darwin" if target == "macos" else target, arch, backend_ver)
            prov = temp / "BACKEND.txt"
            provenance(prov, manifest, revision, backend_ver)
            suffix = ".zip" if target in {"windows", "macos"} else ".tar.gz"
            result = output / f"aiu-cli-{frontend_version}-{target}-{arch}{suffix}"
            archive([binary, backend / "LICENSE", root / "docs" / "platform-support.md", root / "docs" / "attribution.md", prov], result, "aiu", suffix == ".zip")
            outputs.append(result)
    return outputs


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend-source", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path("dist"))
    parser.add_argument("--version", required=True)
    parser.add_argument("--cli-all", action="store_true")
    parser.add_argument("--cli-target", action="append", choices=sorted(CLI_CHOICES))
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", args.version):
        raise SystemExit("frontend version must be X.Y.Z")
    root = Path(__file__).resolve().parents[1]
    backend = args.backend_source.resolve()
    manifest, revision = read_backend_manifest(root, backend)
    backend_ver = backend_version(backend)
    if args.cli_all and args.cli_target:
        raise SystemExit("--cli-all and --cli-target cannot be combined")
    label, arch = native_target()
    targets = list(CLI_TARGETS) if args.cli_all else [CLI_CHOICES[x] for x in (args.cli_target or [])]
    if args.cli_all and label != "macos":
        targets = [target for target in targets if target[0] != "macos"]
    if args.cli_all and label == "macos":
        targets = [target for target in targets if target == (label, arch)]
    package_desktop(root, backend, args.output, args.version, manifest, revision, backend_ver)
    package_cli(root, backend, args.output, args.version, manifest, revision, backend_ver, targets)
    for path in sorted(args.output.iterdir()):
        if path.is_file() and not path.name.endswith(".sha256"):
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            path.with_name(path.name + ".sha256").write_text(f"{digest}  {path.name}\n", encoding="utf-8")
            print(path)


if __name__ == "__main__":
    main()
