# Actuate

Native control for your agents and applications.

Actuate is a native UI automation toolkit. Inspect interfaces, capture screens,
and orchestrate interactions with apps, windows, and desktops. Compose its Rust
providers in your code or use the CLI and persistent sessions.

## Get started

Build from source with Rust and Python 3.9 or newer:

```sh
python scripts/prepare-deps.py
cargo build -p cli --no-default-features -p overlay
cargo run -p cli --no-default-features -- capabilities
```

The minimal build includes the host desktop provider. See the
[installation guide](www/docs/installation.md) for native prerequisites and
optional device features. The [quick start](www/docs/quick-start.md) covers
discovery and persistent sessions.

## Choose an entry point

| Use | Available today | Planned |
| --- | --- | --- |
| Build | [Rust providers](www/docs/build/rust.md) | TypeScript and Python SDKs |
| Automate | [CLI](www/docs/automate/commands.md), [persistent sessions](www/docs/automate/sessions.md) | More session adapters |
| Connect | Run the CLI through local command tools | [MCP, skills, and agent plugins](www/docs/connect/index.md) |

## Platform status

Actuate is experimental. Providers exist for macOS, Linux, Windows, Android, and
iOS and iPadOS devices and simulators. Capabilities depend on the provider,
permissions, and OS. Query `capabilities` before acting.

See the [platform guide](www/docs/platforms/index.md) for native routes and
platform-specific requirements.

## Documentation

- [Quick start](www/docs/quick-start.md)
- [Extend Actuate](www/docs/developers/index.md)
- [Architecture](architecture.md) and [technical specifications](_specs/README.md)
- [Website development](www/README.md), built with [Blume](https://useblume.dev/)
