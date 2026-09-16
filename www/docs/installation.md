---
title: Installation
description: Build the Actuate CLI and cursor helper from source. Check desktop prerequisites and enable optional Android and Apple device providers.
---

Actuate does not publish installable releases or language packages yet. Build
the current source with Rust and Python 3.9 or newer. Use Rust's stable toolchain
with edition 2024 support.

```sh
git clone https://github.com/Rajaniraiyn/actuate.git
cd actuate
python scripts/prepare-deps.py
cargo build -p cli --no-default-features -p overlay
```

The preparation script fetches pinned dependencies and applies reviewed patches.
Cargo resolves those paths even for a desktop-only build.

## Run the binary

On macOS and Linux:

```sh
./target/debug/actuate --help
./target/debug/actuate capabilities
```

On Windows, use a Rust MSVC toolchain with the Visual Studio C++ build tools and
Windows SDK:

```powershell
.\target\debug\actuate.exe --help
.\target\debug\actuate.exe capabilities
```

The examples on this site use `actuate` after adding `target/debug` to your PATH.
The optional cursor helper is `actuate-overlay` in the same directory.

## Platform requirements

macOS observation and input require the appropriate Accessibility permission.
Screen capture requires Screen Recording permission. Simulator operations need
Xcode and an explicitly selected, booted simulator.

Linux needs a running accessibility bus for AT-SPI. Input, capture, and overlays
depend on X11 or supported Wayland protocols. See the [platform guide](/platforms).

The minimal command above excludes optional Apple physical-device and Android
dependencies. Build `cargo build -p cli` to include the default device features;
connection setup and native prerequisites remain provider-specific.

Continue with the [quick start](/quick-start).
