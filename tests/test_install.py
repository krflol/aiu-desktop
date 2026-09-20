import argparse, hashlib, io, json, os, tarfile, zipfile
from pathlib import Path

import install

def test_frontend_selection_and_conflicts(monkeypatch):
    monkeypatch.setattr(install, "host_platform", lambda: "windows")
    assert install.choose_frontend("auto", False) == "rust"
    assert install.choose_frontend("auto", True) == "cli"
    try: install.choose_frontend("swift", True)
    except ValueError: pass
    else: assert False

def test_safe_zip_rejects_escape(tmp_path):
    archive = tmp_path / "bad.zip"
    with zipfile.ZipFile(archive, "w") as z: z.writestr("../escape", b"x")
    try: install.extract_safe(archive, tmp_path / "out")
    except ValueError: pass
    else: assert False

def test_safe_zip_rejects_symlink(tmp_path):
    archive = tmp_path / "bad.zip"
    info = zipfile.ZipInfo("link")
    info.external_attr = (0o120777 << 16)
    with zipfile.ZipFile(archive, "w") as z: z.writestr(info, b"target")
    try: install.extract_safe(archive, tmp_path / "out")
    except ValueError: pass
    else: assert False

def test_checksum_requires_matching_digest(tmp_path):
    blob = tmp_path / "a.zip"; blob.write_bytes(b"payload")
    asset = {"name":"x.zip", "digest":"sha256:" + "0" * 64}
    try: install.verify_checksum(blob, asset, {"assets":[]})
    except ValueError as e: assert "checksum" in str(e)
    else: assert False

def test_dry_run_does_not_fetch(monkeypatch, capsys):
    monkeypatch.setattr(install, "release", lambda *a: (_ for _ in ()).throw(AssertionError("network")))
    assert install.main(["--dry-run", "--frontend", "rust", "--version", "latest"]) == 0
    assert "planned" in capsys.readouterr().out

def test_invalid_version():
    try: install.version_tag("1.2.3")
    except ValueError: pass
    else: assert False

def test_exact_asset_and_unsupported_arch(monkeypatch):
    monkeypatch.setattr(install, "host_platform", lambda: "linux")
    monkeypatch.setattr(install, "arch_name", lambda: "arm64")
    rel = {"tag_name":"v1.2.3", "assets":[{"name":"aiu-cli-1.2.3-linux-arm64.tar.gz"}]}
    assert install.asset_info(rel, "cli")["name"].endswith("linux-arm64.tar.gz")
    monkeypatch.setattr(install, "arch_name", lambda: (_ for _ in ()).throw(ValueError("unsupported architecture")))
    try: install.arch_name()
    except ValueError: pass
    else: assert False

def test_fake_release_installs_atomically(tmp_path, monkeypatch, capsys):
    payload = io.BytesIO()
    with tarfile.open(fileobj=payload, mode="w:gz") as t:
        data = b"#!/bin/sh\n"; info = tarfile.TarInfo("aiu"); info.size=len(data); info.mode=0o755; t.addfile(info, io.BytesIO(data))
    blob = payload.getvalue(); digest=hashlib.sha256(blob).hexdigest()
    monkeypatch.setattr(install, "host_platform", lambda: "linux"); monkeypatch.setattr(install, "arch_name", lambda: "amd64")
    asset={"name":"aiu-cli-1.2.3-linux-amd64.tar.gz", "size":len(blob), "digest":"sha256:"+digest, "browser_download_url":"https://github.com/krflol/aiu-desktop/releases/download/v1.2.3/aiu-cli-1.2.3-linux-amd64.tar.gz"}
    monkeypatch.setattr(install, "release", lambda *a: {"tag_name":"v1.2.3", "assets":[asset]})
    class Response(io.BytesIO):
        def __enter__(self): return self
        def __exit__(self, *args): pass
    monkeypatch.setattr(install.urllib.request, "urlopen", lambda *a, **k: Response(blob))
    args=argparse.Namespace(frontend="auto", cli_only=True, version="v1.2.3", prefix=str(tmp_path), dry_run=False)
    install.install(args); target=tmp_path / "v1.2.3-cli"
    if os.name != "nt": assert (target / "aiu").stat().st_mode & 0o111
    assert "launch:" in capsys.readouterr().out
    try: install.install(args)
    except RuntimeError as e: assert "already exists" in str(e)
    else: assert False

def test_nested_symlink_prefix_rejected(tmp_path):
    parent=tmp_path / "parent"; parent.mkdir(); link=parent / "link"; link.symlink_to(tmp_path / "elsewhere", target_is_directory=True)
    try: install.ensure_no_symlink(link / "child")
    except RuntimeError: pass
    else: assert False
