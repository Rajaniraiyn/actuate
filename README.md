# Actuate

Native control for your agents and applications.

Actuate is a native UI automation toolkit. Inspect interfaces, capture screens,
and interact with apps, windows, and desktops. Compose providers in Rust, build
with TypeScript or Python, or automate through the CLI and persistent sessions.

## Get started

Download the archive for your platform from [GitHub Releases](https://github.com/Rajaniraiyn/actuate/releases)
and put `actuate` on your PATH. The executable includes the cursor renderer as
`actuate overlay`.

```sh
actuate capabilities
actuate discover
actuate session --json
```

Choose an application, observe its accessibility tree, and use the actions it
exposes. Keep a session open when later actions need references from an observation.
The [quick start](https://rajaniraiyn.dev/actuate/quick-start/) walks through this workflow.

## Build

Use the same native providers from your application.

| Language | API | Guide |
| --- | --- | --- |
| Rust | Capability traits and composable backends | [Rust](www/docs/build/rust.md) |
| TypeScript | Typed requests, promises, and explicit session ownership | [TypeScript](www/docs/build/typescript.md) |
| Python | Dataclasses, context managers, and synchronous or asynchronous sessions | [Python](www/docs/build/python.md) |

TypeScript uses napi-rs and Python uses PyO3. Both retain native state on a
session worker and preserve provider errors, reference lifetimes, and input
routes. Custom transports and middleware let applications add policy and tracing.

## Automate

Discover applications and windows, read bounded accessibility trees, capture
screens, and compare observations. Select semantic actions or an explicit input
route. Window-directed and global input have different focus behavior; a soft
cursor visualizes an action without changing its delivery route.

See the [command reference](www/docs/automate/commands.md),
[session protocol](www/docs/automate/sessions.md), and
[input guide](www/docs/guides/input-and-cursors.md).

## Connect

Agents can use local commands or keep a JSONL session open. MCP, packaged skills,
and dedicated agent plugins are planned. The
[integration guide](www/docs/connect/index.md) covers available entry points.

## Platforms

Providers support Windows, macOS, Linux, Android, and Apple devices and simulators.
Operations depend on native permissions, the application, and the selected
provider. Query capabilities before choosing an input route. Read the
[platform guide](www/docs/platforms/index.md) for requirements and limits.

## Development

Mise pins Rust, Bun, Python, and uv. From a checkout, run:

```sh
mise install
mise run setup
mise run stage
mise run test
```

Use `mise run docs` to build the documentation and `mise run release:build` to
build distribution binaries.

## Build from source

```sh
git clone https://github.com/Rajaniraiyn/actuate.git
cd actuate
cargo build -p cli --release --locked
./target/release/actuate --help
```

On Windows, run `target\release\actuate.exe`. Use `--no-default-features` for a
desktop-only build. Cargo resolves the checked-in dependency overrides without a
preparation script. See [installation](www/docs/installation.md) for native build
requirements and [binding development](_specs/bindings.md) for language packages.

[Documentation](https://rajaniraiyn.dev/actuate/) ?
[Extend Actuate](www/docs/developers/index.md) ?
[Technical specifications](_specs/README.md)
