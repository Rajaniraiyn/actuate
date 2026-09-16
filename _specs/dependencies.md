# Patched Android dependencies

Android uses DroidMux for ADB transport, shell and native wireless pairing. Three upstream crates need local fixes. The repository stores unified patch files, a source revision and the archive SHA-256 in [patches/manifest.json](../patches/manifest.json). It does not store dependency source trees.

Prepare a checkout once before running Cargo:

```sh
python3 scripts/prepare-deps.py
cargo build
```

Python 3.9 or newer and Git are required for preparation. Cargo then reads the generated crates under `target/patched-deps/`. Repeat preparation after pulling changed patches or running `cargo clean`. An unchanged prepared tree needs no network access. The script refuses to overwrite modified generated files; preserve your changes before removing that directory yourself.

For offline preparation, download the manifest's archive URL in advance, then run:

```sh
python3 scripts/prepare-deps.py --archive /path/to/droidmux.tar.gz
cargo build --offline
```

Cargo's other dependencies must also be cached. An optional snapshot can be generated after preparation:

```sh
mkdir -p target/offline
cargo vendor --locked --versioned-dirs "$(pwd)/target/offline/sources" > target/offline/config.toml
cargo build --offline --locked --config target/offline/config.toml
```

The absolute output path avoids Cargo interpreting the source directory relative to the configuration file. This snapshot is a local build artifact; regenerate its configuration if the checkout moves. Patched crates remain under `target/patched-deps`, so keep that directory alongside the snapshot for offline builds.

## Maintaining patches

Each patch applies to one pristine crate at the pinned upstream revision. The preparation step copies the crate and upstream license files, then uses `git apply --check` and `git apply`. Patches include standalone Cargo manifests so their dependencies resolve to the same upstream revision and the workspace's overrides.

To change a patch, work in a separate copy of the generated source, compare it with the pinned pristine crate, and regenerate its unified diff with paths rooted at `droidmux-client/`, `droidmux-shell/` or `droidmux-pairing/`. Validate from an empty generated directory. Do not edit the only copy of generated source and expect preparation to discard it.

The patches preserve upstream licensing. Their `ACTUATE.md` additions describe the fixes and relevant source references. Remove a patch when an upstream release contains the required behavior and passes the same regression and device tests.
