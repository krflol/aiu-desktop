# Platform support

Go is the sole backend. Its exact source revision is pinned in backend.json and
recorded in BACKEND.txt inside every bundle. Rust owns presentation and process
lifecycle; the upstream repository owns Go and SwiftUI.

| System | Rust desktop | Standalone Go CLI |
| --- | --- | --- |
| Windows 10/11 amd64 | Native build/tests, ZIP | Native tests, ZIP |
| Windows ARM64 | Not packaged | Cross-built ZIP |
| Linux amd64 | Ubuntu 22.04 build/tests, tar.gz | Native tests, tar.gz |
| Linux ARM64 | Not packaged | Cross-built tar.gz |
| macOS arm64 | Native build/tests, app ZIP | Native tests, ZIP |
| macOS amd64 | Native build/tests, app ZIP | Native tests, ZIP |

Archives use aiu-desktop-VERSION-OS-ARCH or aiu-cli-VERSION-OS-ARCH, with
windows, linux, or macos and amd64 or arm64. Each has a .sha256 sidecar. Native
macOS Go builds retain cgo for Keychain. Windows/Linux CLI binaries need no GUI.

## Runtime requirements

Linux desktop needs GTK 3, Ayatana AppIndicator 3, xkbcommon for X11, a session
bus, and a graphical display. Ubuntu packages are libgtk-3-0,
libayatana-appindicator3-1, and libxkbcommon-x11-0; source
builds additionally need libgtk-3-dev and libayatana-appindicator3-dev. A
StatusNotifier host enables close-to-tray. Without one, the application stays
reachable and closing its window quits. CI tests both cases with Xvfb, Openbox,
and a synthetic DBus host; this does not certify every desktop extension.

Rust macOS bundles are ad-hoc signed, not Developer ID signed or notarized.
They remain a developer option where Gatekeeper blocks downloaded applications.
The default macOS installer choice uses upstream SwiftUI through Homebrew and
retains upstream's macOS version requirements and installation paths.

## Installation and storage

The Python 3.9+ installer supports --frontend auto|rust|swift, --cli-only,
--version vX.Y.Z, --prefix PATH, and a dry run that performs no network or writes.
Swift delegates to Homebrew and does not accept a custom version or prefix.
Rust/CLI archives are checksum-verified and installed into new versioned
folders. Existing installations are never overwritten. The installer prints
how to launch the component and does not change PATH, registry, or account data.

AIU_CONFIG_DIR overrides the Go store. Existing ~/.config/aiu stores remain
authoritative. New Windows stores use the user's roaming configuration folder;
Linux honors an absolute XDG_CONFIG_HOME; macOS keeps upstream's location.
Windows defaults to current-user DPAPI and protected ACLs, macOS to Keychain,
and Linux to owner-only files. AIU_STORE=file explicitly chooses file storage.
The original full Rust fork's aiu-rs store is separate and is not migrated.
All imports, external CLI writes, cache limits, refresh ownership, and
recommendations are handled by Go; the frontend implements no second policy.

## Verification

Go tests use isolated stores and synthetic provider servers. Rust contract tests
consume Go-produced fixtures, and process tests check cancellation and reaping.
On Windows, scripts/test-tray.ps1 verifies native tray creation, close-to-tray,
hide/show, refresh, and quit; repeat with -OccludedQuit to test a covered window.
Linux scripts/test-linux-desktop.sh verifies the tray and no-host paths.
Consult CI runs and release notes for results at a particular revision.

Live-provider browser approval, running external CLI interoperability, and
individual Linux desktop tray integrations still require manual verification.
Fixture tests do not claim these manual checks have passed. Use isolated
AIU_CONFIG_DIR, CLAUDE_CONFIG_DIR, and CODEX_HOME directories and never put real
credentials or authorization codes in test fixtures or logs.
