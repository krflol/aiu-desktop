# AIU desktop

`aiu-desktop` is a Rust desktop and tray frontend for Windows, Linux, and macOS.
**Go owns every backend operation**: credentials, OAuth, usage polling, caching,
account switching, and recommendations. Rust displays presentation data and
invokes the bundled Go executable.

The [upstream project](https://github.com/getparable/aiu) maintains Go and its
native SwiftUI macOS app. This repository independently maintains the Rust
frontend and its releases. The complete original Rust implementation remains
available in [aiu-rs](https://github.com/krflol/aiu-rs).

## Choose a frontend

Download `install.py` from a [release](https://github.com/krflol/aiu-desktop/releases)
or clone this repository. It needs Python 3.9+; on Windows use `py -3` instead of
`python3`.

```sh
python3 install.py --dry-run
python3 install.py                       # Swift on macOS; Rust on Windows/Linux
python3 install.py --frontend rust       # Rust desktop, including on macOS
python3 install.py --frontend swift      # Upstream SwiftUI through Homebrew
python3 install.py --cli-only            # Go CLI without a desktop
```

Rust and CLI installs verify SHA-256 and extract into a new versioned directory.
Use `--version v0.1.0` or `--prefix PATH` to choose the release and destination.
Swift uses Homebrew's paths and version management, so those options do not
apply. The installer does not modify account stores, PATH, or Windows registry
settings. Keep the bundled `aiu` and `aiu-desktop` executables together.

## Backend and development

[`backend.json`](backend.json) pins the exact Go source used by packaging.
[Upstream PR #11](https://github.com/getparable/aiu/pull/11) merged the portable Go
backend. The pin includes [banked reset support in upstream PR #13](https://github.com/getparable/aiu/pull/13)
based on upstream v0.2.0. Packages include its provenance
in `BACKEND.txt`. The [versioned process contract](docs/frontend-contract.md)
keeps frontend releases independent of backend implementation details.

Cancel and Quit close pending browser waits and allow an already-started Go
credential exchange to finish saving. Closing a browser tab itself cannot be
detected. Close-to-tray is available when a tray host exists; without a Linux
host the window remains reachable and closing it quits.

## Banked Codex resets

Codex cards show the reset balance and cached credit details. **Details / refresh**
loads the provider's credit list. **Use a reset** asks for confirmation; uncertain
results offer **Retry same request** to avoid spending another credit.

Each Codex account has an **Auto reset at 1% remaining or less** toggle, off by
default. The shared Go backend checks fresh usage, uses an available banked reset,
and waits for usage to recover below the threshold before another automatic
spend. It works while desktop/tray, CLI status, or watch collects usage, subject
to the shared polling and provider cooldowns. Enabling it authorizes consumption
of banked resets, which reset eligible five-hour/weekly limits and move the weekly
reset date. See [reset policy and recovery](docs/banked-resets.md).

## Build from source

Build and test with Rust 1.88:

```sh
cargo +1.88.0 test --locked --all-features
cargo +1.88.0 build --locked --release --bin aiu-desktop
python3 -m pytest -q tests/test_install.py
```

The fixture interface is useful for UI work without credentials:

```sh
./target/release/aiu-desktop --fixture tests/fixtures/frontend/accounts.json
```

Build the pinned Go source and put its executable beside the Rust binary. There
is no PATH lookup or environment override for selecting a backend. Fixture mode
never reads real accounts. To package a pinned checkout:

```sh
python3 scripts/package-desktop.py --backend-source ../aiu-go-windows --version 0.2.0 --output dist
```

Native desktop bundles target Windows/Linux amd64 and macOS amd64/arm64. See
[`docs/platform-support.md`](docs/platform-support.md) for dependencies, release
assets, backend provenance, and smoke testing. The original Rust contribution
is credited in [attribution](docs/attribution.md) under the MIT license.
