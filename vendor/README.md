# Vendored DroidMux sources

DroidMux provides the direct Android transport. These sources come from
[poapoauu/DroidMux](https://github.com/poapoauu/DroidMux) at revision
`d21e3d729a9d233fd34ce27e5c2244c45c4c9a8e`, under MIT OR Apache-2.0.
Each crate retains its upstream license files.

`sources/` contains the unchanged directory source produced by `cargo vendor
--versioned-dirs`. `.cargo/config.toml` replaces only the pinned DroidMux Git
source. Registry packages continue to resolve through crates.io and Cargo.lock.

`patches/` contains the three reviewed local overrides. Cargo's `[patch]` entries
select them directly. Their diffs and upstream archive identity are recorded in
`patches/` at the repository root. Do not edit directory sources or regenerate
their checksums to hide a patch; edit the local override instead.

To refresh, run `cargo vendor --locked --versioned-dirs target/vendor-refresh`,
copy the DroidMux directory-source crates into `vendor/sources`, and review the
local patches against the new revision. No consumer preparation script runs.

The core `actuate` crate has only registry dependencies. The Android provider
still depends on unpublished upstream packages and remains `publish = false`.
Vendoring makes checkout builds reproducible; it does not make Git dependencies
publishable on crates.io.
