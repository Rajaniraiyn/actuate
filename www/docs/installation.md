---
title: Installation
description: Install the Actuate executable from GitHub Releases or build it with Cargo. Set up native language bindings and check platform requirements.
---

## Download a binary

Choose an archive from [GitHub Releases](https://github.com/Rajaniraiyn/actuate/releases)
for your operating system and architecture. Extract it and add its directory to
PATH. Each archive contains one executable, including the `overlay` subcommand.
Compare the archive's SHA-256 with `SHA256SUMS` from the same release.

```sh
actuate --help
actuate capabilities
```

Release builds target Windows x64 and ARM64, macOS x64 and Apple silicon, and Linux x64 and
ARM64 with glibc 2.35 or newer. Other targets need a source build. Private releases
require repository access.

## Build from source

Use stable Rust with edition 2024 support. The bindings require Rust 1.88 or newer.

```sh
git clone https://github.com/Rajaniraiyn/actuate.git
cd actuate
cargo build -p cli --release --locked
```

Cargo uses the vendored dependency sources and reviewed overrides directly.
Add `--no-default-features` to exclude Android and Apple physical-device support.

On macOS and Linux, run `./target/release/actuate`. On Windows, use a Rust MSVC
toolchain with the Visual Studio C++ build tools and Windows SDK, then run
`.\target\release\actuate.exe`.

## Language packages

Release assets include a TypeScript package archive and Python wheels and source
package. See the [TypeScript](/build/typescript) and [Python](/build/python) guides
for installation and session ownership. Native binaries come from the same
versioned GitHub release as the language package.

## Platform requirements

macOS observation and input require Accessibility permission. Screen capture
requires Screen Recording permission. Simulator operations need Xcode and an
explicitly selected, booted simulator.

Linux observation needs a desktop accessibility bus. Input, capture, and overlays
depend on X11 or supported Wayland protocols. Android and Apple device connections
have their own pairing and permission requirements. See [platforms](/platforms).

Continue with the [quick start](/quick-start).
