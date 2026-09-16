# Actuate

Native control for your agents and applications.

Actuate is a native UI automation toolkit. Inspect interfaces, capture screens,
and orchestrate interactions with apps, windows, and desktops. Compose its Rust
providers in your code or use the CLI and persistent sessions.

SDKs, CLI, MCP, skills, and plugins are ways to access the same composable core.
Rust and the CLI are available today. TypeScript, Python, MCP, and packaged agent
integrations are planned.

## Get started

Build from source with Rust and Python 3.9 or newer:

```sh
git clone https://github.com/Rajaniraiyn/actuate.git
cd actuate
python scripts/prepare-deps.py
cargo build -p cli --no-default-features -p overlay
cargo run -p cli --no-default-features -- capabilities
cargo run -p cli --no-default-features -- discover
```

The minimal build includes the host desktop provider. See the
[installation guide](www/docs/installation.md) for native prerequisites and
optional device features. Packages are not published to language registries yet.

Run `target/debug/actuate session --json` to reuse connections and element
references. Windows executables have an `.exe` suffix. Output is text by default;
add `--json` for structured output.

## Choose an entry point

| Use | Available today | Planned |
| --- | --- | --- |
| Build | [Rust providers](www/docs/build/rust.md) | TypeScript and Python SDKs |
| Automate | [CLI](www/docs/automate/commands.md), [persistent sessions](www/docs/automate/sessions.md) | More session adapters |
| Connect | Run the CLI through local command tools | [MCP, skills, and agent plugins](www/docs/connect/index.md) |

Adapters preserve capability reporting, target identity, action semantics, and
structured errors. They must not silently replace a window-targeted request with
global input.

## Platform status

Actuate is experimental. Providers exist for macOS, Linux, Windows, Android, and
iOS and iPadOS devices and simulators. Capabilities depend on the provider,
permissions, and OS. Query `capabilities` before acting.

Windows tests cover native app fixtures, Save/Open dialogs, shell interactions,
cursor overlays, and Explorer transfers. Semantic actions can activate windows.
Full shell coverage, workspace isolation, and background focus safety remain
incomplete. Read the [validation reports](_specs/validation.md) and
[Windows results](_specs/windows-interactive-results.md).

## Documentation

- [Quick start](www/docs/quick-start.md)
- [CLI and language API design](www/docs/design/index.md)
- [Architecture](architecture.md) and [technical specifications](_specs/README.md)
- [Website development](www/README.md), built with [Blume](https://useblume.dev/)
