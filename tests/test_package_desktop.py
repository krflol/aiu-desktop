import importlib.util
import tarfile
import zipfile
from pathlib import Path


ROOT = Path(__file__).parents[1]
SPEC = importlib.util.spec_from_file_location("package_desktop", ROOT / "scripts" / "package-desktop.py")
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


def test_backend_version_is_read_from_makefile(tmp_path):
    (tmp_path / "Makefile").write_text("VERSION   ?= 0.1.3\n", encoding="utf-8")
    assert PACKAGE.backend_version(tmp_path) == "0.1.3"


def test_archive_preserves_expected_prefix_and_executable_mode(tmp_path):
    binary = tmp_path / "aiu"
    binary.write_bytes(b"binary")
    archive = tmp_path / "bundle.tar.gz"
    PACKAGE.archive([binary], archive, "aiu", False)
    with tarfile.open(archive, "r:gz") as bundle:
        member = bundle.getmember("aiu/aiu")
        assert member.mode & 0o777 == 0o755


def test_archive_zip_does_not_duplicate_app_prefix(tmp_path):
    app = tmp_path / "AIU.app"
    app.mkdir()
    (app / "Contents").mkdir()
    (app / "Contents" / "MacOS").mkdir()
    (app / "Contents" / "MacOS" / "aiu-desktop").write_bytes(b"binary")
    archive = tmp_path / "bundle.zip"
    PACKAGE.archive([app], archive, "aiu-desktop", True)
    with zipfile.ZipFile(archive) as bundle:
        assert "aiu-desktop/AIU.app/Contents/MacOS/aiu-desktop" in bundle.namelist()
        assert "aiu-desktop/aiu-desktop/AIU.app" not in bundle.namelist()
