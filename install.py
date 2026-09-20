#!/usr/bin/env python3
"""Install AIU desktop/CLI releases without changing accounts or PATH."""
import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path, PurePosixPath

REPO = "krflol/aiu-desktop"
MAX_DOWNLOAD = 512 * 1024 * 1024

def host_platform():
    s = sys.platform
    if s.startswith("win"): return "windows"
    if s == "darwin": return "macos"
    if s.startswith("linux"): return "linux"
    raise ValueError(f"unsupported platform: {s}")

def arch_name():
    m = platform.machine().lower()
    if m in ("amd64", "x86_64"): return "amd64"
    if m in ("arm64", "aarch64"): return "arm64"
    raise ValueError(f"unsupported architecture: {m}")

def default_prefix():
    if host_platform() == "windows": return Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData/Local")) / "Programs" / "AIU"
    return Path.home() / ".local" / "share" / "aiu"

def version_tag(value, dry_run=False):
    if value == "latest": return "latest"
    if not value.startswith("v") or value.count(".") != 2 or any(not x.isdigit() for x in value[1:].split(".")):
        raise ValueError("version must be latest or vX.Y.Z")
    return value

def choose_frontend(name, cli_only):
    if cli_only and name != "auto": raise ValueError("--cli-only cannot be combined with an explicit --frontend")
    if cli_only: return "cli"
    if name == "auto": return "swift" if host_platform() == "macos" else "rust"
    if name not in ("rust", "swift"): raise ValueError("frontend must be auto, rust, or swift")
    if name == "swift" and host_platform() != "macos": raise ValueError("Swift frontend is supported on macOS only")
    return name

def request_json(url):
    req = urllib.request.Request(url, headers={"Accept":"application/vnd.github+json", "User-Agent":"aiu-installer"})
    with urllib.request.urlopen(req, timeout=30) as r:
        data = r.read(2 * 1024 * 1024 + 1)
        if len(data) > 2 * 1024 * 1024: raise ValueError("GitHub metadata response is too large")
        return json.loads(data)

def release(repo, tag):
    return request_json(f"https://api.github.com/repos/{repo}/releases/{'latest' if tag == 'latest' else 'tags/'+tag}")

def asset_info(rel, kind):
    plat, arch = host_platform(), arch_name()
    prefix = "aiu-cli" if kind == "cli" else "aiu-desktop"
    tag = rel.get("tag_name", "")
    if not re.fullmatch(r"v\d+\.\d+\.\d+", tag): raise ValueError("release tag is not a stable vX.Y.Z release")
    ext = ".tar.gz" if plat == "linux" else ".zip"
    wanted = f"{prefix}-{tag[1:]}-{plat}-{arch}{ext}"
    for a in rel.get("assets", []):
        if a.get("name") == wanted: return a
    raise ValueError(f"release has no exact {wanted} asset")

def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1024 * 1024), b""): h.update(b)
    return h.hexdigest()

def verify_checksum(path, asset, rel):
    digest = asset.get("digest", "")
    if digest.startswith("sha256:"): expected = digest.split(":", 1)[1]
    else:
        side = next((a for a in rel.get("assets", []) if a.get("name") == asset.get("name") + ".sha256"), None)
        if not side: raise ValueError("release asset has no SHA-256 digest or sidecar")
        validate_asset_url(side.get("browser_download_url", ""), rel.get("tag_name", ""), asset["name"] + ".sha256")
        if int(side.get("size", 0)) <= 0 or int(side.get("size", 0)) > 1024 * 1024: raise ValueError("checksum sidecar size is invalid")
        text = urllib.request.urlopen(urllib.request.Request(side["browser_download_url"], headers={"User-Agent":"aiu-installer"}), timeout=30).read().decode()
        expected = text.split()[0].strip()
    if not re.fullmatch(r"[0-9a-fA-F]{64}", expected): raise ValueError("release SHA-256 digest is invalid")
    actual = sha256_file(path)
    if expected.lower() != actual.lower(): raise ValueError("SHA-256 checksum mismatch")

def safe_members(names):
    for raw in names:
        if raw.startswith(("/", "\\")) or re.match(r"^[A-Za-z]:", raw) or raw.startswith(("\\\\", "//")):
            raise ValueError(f"unsafe archive path: {raw}")
        p = PurePosixPath(raw.replace("\\", "/"))
        if p.is_absolute() or ".." in p.parts or not p.parts or any(":" in part for part in p.parts): raise ValueError(f"unsafe archive path: {raw}")

def ensure_no_symlink(path):
    p = Path(path).absolute()
    current = Path(p.parts[0])
    for part in p.parts[1:]:
        current = current / part
        try:
            if current.is_symlink(): raise RuntimeError(f"refusing symlinked install path: {current}")
        except OSError as e: raise RuntimeError(f"cannot inspect install path: {current}: {e}")

def validate_asset_url(url, tag, name):
    u = urllib.parse.urlparse(url)
    expected = f"https://github.com/{REPO}/releases/download/{tag}/{name}"
    if u.scheme != "https" or u.netloc != "github.com" or u.path != urllib.parse.urlparse(expected).path or u.query or u.fragment:
        raise ValueError("release asset URL is outside the expected GitHub release")

def extract_safe(archive, destination):
    ensure_no_symlink(destination.parent)
    destination.mkdir(parents=True, exist_ok=True)
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as z:
            safe_members([x.filename for x in z.infolist()])
            for x in z.infolist():
                mode = (x.external_attr >> 16) & 0o170000
                if mode == stat.S_IFLNK: raise ValueError("archive symlinks are not allowed")
                out = destination / PurePosixPath(x.filename)
                if x.is_dir(): out.mkdir(parents=True, exist_ok=True); continue
                out.parent.mkdir(parents=True, exist_ok=True)
                with z.open(x) as src, open(out, "wb") as dst: shutil.copyfileobj(src, dst)
                perms = (x.external_attr >> 16) & 0o777
                if perms: os.chmod(out, perms)
    else:
        with tarfile.open(archive, "r:gz") as t:
            safe_members([x.name for x in t.getmembers()])
            for x in t.getmembers():
                if not (x.isdir() or x.isfile()): raise ValueError("archive links and special files are not allowed")
            t.extractall(destination)

def brew_plan(version):
    if version != "latest": raise ValueError("Swift/Homebrew installs accept only --version latest")
    brew = shutil.which("brew")
    if not brew: raise RuntimeError("Homebrew is required for the Swift frontend; install it from https://brew.sh")
    return [brew, "upgrade", "getparable/tap/aiu"] if subprocess.run([brew,"list","aiu"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0 else [brew,"install","getparable/tap/aiu"]

def install(args):
    kind = choose_frontend(args.frontend, args.cli_only); tag = version_tag(args.version, args.dry_run); prefix = Path(args.prefix).expanduser() if args.prefix else default_prefix()
    if kind == "swift":
        if args.prefix: raise ValueError("--prefix is unsupported for Swift/Homebrew installs")
        if args.version != "latest": raise ValueError("Swift/Homebrew installs accept only --version latest")
        if args.dry_run: print("planned: brew install getparable/tap/aiu"); return
        cmd = brew_plan(tag); print("planned:", " ".join(cmd));
        if not args.dry_run: subprocess.run(cmd, check=True)
        return
    if args.dry_run:
        print(f"planned {kind} {tag} asset for {host_platform()}-{arch_name()} into {prefix / (tag + '-' + kind)}"); return
    rel = release(REPO, tag); asset = asset_info(rel, kind)
    size = int(asset.get("size", 0));
    if size <= 0 or size > MAX_DOWNLOAD: raise RuntimeError("release asset size is missing or exceeds installer limit")
    release_tag = rel.get("tag_name") or tag
    target = prefix / f"{release_tag}-{kind}"
    ensure_no_symlink(prefix); prefix.mkdir(parents=True, exist_ok=True); ensure_no_symlink(prefix)
    if target.exists(): raise RuntimeError(f"install target already exists: {target}")
    with tempfile.TemporaryDirectory() as td:
        archive = Path(td) / asset["name"]
        validate_asset_url(asset.get("browser_download_url", ""), release_tag, asset["name"])
        req = urllib.request.Request(asset["browser_download_url"], headers={"Accept":"application/octet-stream", "User-Agent":"aiu-installer"})
        with urllib.request.urlopen(req, timeout=60) as src, open(archive, "wb") as dst:
            total=0
            for chunk in iter(lambda: src.read(1024*1024), b""):
                total += len(chunk)
                if total > MAX_DOWNLOAD: raise RuntimeError("download exceeds installer limit")
                dst.write(chunk)
        verify_checksum(archive, asset, rel)
        with tempfile.TemporaryDirectory(prefix=".aiu-stage-", dir=str(prefix)) as staging:
            stage = Path(staging) / "payload"
            extract_safe(archive, stage)
            if target.exists():
                raise RuntimeError(f"install target already exists: {target}")
            try:
                os.rename(stage, target)
            except FileExistsError:
                raise RuntimeError(f"install target already exists: {target}")
    print(f"installed {target}\nlaunch: {launch_command(target, kind)}")

def launch_command(target, kind):
    if kind == "swift": return "brew services info getparable/tap/aiu"
    apps = sorted(target.rglob("*.app"))
    if apps: return f'open "{apps[0]}"'
    names = ["aiu-desktop.exe", "aiu-desktop"] if kind == "rust" else ["aiu.exe", "aiu"]
    for name in names:
        found = next(target.rglob(name), None)
        if found and found.is_file():
            return f'& "{found}"' if host_platform() == "windows" else f'"{found}"'
    return f'cd "{target}" and run the installed executable'

def main(argv=None):
    p=argparse.ArgumentParser(); p.add_argument("--frontend", choices=["auto","rust","swift"], default="auto"); p.add_argument("--cli-only", action="store_true"); p.add_argument("--version", default="latest"); p.add_argument("--prefix"); p.add_argument("--dry-run", action="store_true")
    try: install(p.parse_args(argv)); return 0
    except (ValueError, RuntimeError, OSError, subprocess.CalledProcessError, urllib.error.URLError) as e: print(f"error: {e}", file=sys.stderr); return 2
if __name__ == "__main__": raise SystemExit(main())
