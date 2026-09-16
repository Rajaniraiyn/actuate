# Unimation

Give your agent the controls.

Composable UI automation in Rust, with a CLI for agents. Inspect interfaces, take
screenshots, click, type, scroll, and compare snapshots.

macOS, Linux, Android, iOS, and iPadOS, with Windows reserved. Experimental;
capabilities vary by platform and are reported by `capabilities`.

## Get started

```sh
python3 scripts/prepare-deps.py
cargo build -p cli
cargo run -p cli -- --help
cargo run -p cli -- discover
```

Output is compact text by default. Add `--json` for structured output. Keep
`unimation session --json` running to reuse connections and element references.

## Docs

[Architecture](architecture.md) · [Provider composition](docs/composition.md) ·
[Session protocol](docs/session.md) · [Platform guides](docs/backends.md) ·
[Linux](docs/linux.md) · [Validation](docs/validation.md)
