# Dependency sources

Cargo builds a checkout without running a preparation script.
`.cargo/config.toml` replaces the pinned DroidMux Git source with the unchanged
`cargo vendor` output in `vendor/sources`. Registry dependencies remain locked
through Cargo.lock. Three `[patch]` overrides point at `vendor/patches`.

Upstream source identity, licenses, and refresh instructions are in
[the vendor guide](../vendor/README.md). Reviewed patches remain in `patches/`.
Directory sources are immutable; changes belong in a path override.

The core `actuate` crate only uses registry dependencies. Mobile provider crates
remain unpublished while their upstream dependencies are Git-based. Publishing a
library needs its own dependency audit; workspace patches are not inherited by
consumers of a published crate.
