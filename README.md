# Unimation

Agent-first UI automation library. The initial Cargo workspace contains portable capability traits, direct macOS providers, and a CLI declared with [jdx/usage](https://github.com/jdx/usage). The [architecture](architecture.md) describes the full design. This implementation is its first working slice.

## Build

```sh
cargo build --workspace
cargo run -p unimation-cli -- --help
cargo run -p unimation-cli -- discover
cargo run -p unimation-cli -- observe 1234 --max-nodes 1000 --max-depth 30
cargo run -p unimation-cli -- spec
cargo run -p unimation-cli -- protocol
```

macOS requires an installed Apple SDK and linker. If the default Xcode selection is unusable but Command Line Tools are installed, prefix build commands with `DEVELOPER_DIR=/Library/Developer/CommandLineTools`.

`discover` reports actual accessibility trust without prompting. Permission inheritance depends on the host and launch context; the backend checks it at runtime.

## Embedding

`unimation-core` has independent `Observe`, `SemanticActions`, `PointerInput`, `TextInput`, and `Discover` traits. `Capture`, `KeyboardInput`, `WindowControl`, and `Subscribe` define extension points. `unimation-macos` exposes `Accessibility` and `QuartzInput` through modern `objc2` framework bindings. It has no Swift runtime or helper dependency. Other platform crates are reserved for implementation.

## Agent sessions

`unimation session` reads one JSON request per line and writes one JSON response per line. Keep the process alive to retain references. One-shot `observe` results are historical; their references cannot be used in another process.

```json
{"op":"discover"}
{"op":"observe","request":{"pid":1234,"max_nodes":1000,"max_depth":30}}
{"op":"attribute","target":{"session":"SESSION","id":12},"name":"AXRole"}
{"op":"semantic","target":{"session":"SESSION","id":12},"action":{"kind":"perform","name":"AXPress"}}
{"op":"pointer","delivery":{"kind":"process","pid":1234},"action":{"kind":"click","point":{"x":200.0,"y":300.0}}}
{"op":"text","delivery":{"kind":"process","pid":1234},"text":"hello"}
```

Use returned references, never guessed IDs. Input requires an explicit `global` or `process` delivery route. Points currently use Quartz desktop coordinates. No capture-coordinate conversion is implemented yet. Unicode text packets are distinct from physical keys and IME composition. A `dispatched` receipt does not prove that an app consumed an event.

## Current limits

Snapshots enumerate every advertised attribute and preserve read errors. They include action and parameterized-attribute names. Traversal follows `AXChildren`; other relationships remain in raw attributes. Node/depth budgets and failed reads mark snapshots incomplete. `traversal_complete` distinguishes finished traversal from attribute coverage; expected missing optional values retain their native results. Reads are sequential and are not an atomic view of the desktop.

Unmapped values remain retained in the provider and are exposed as session-scoped opaque handles. Rust embeddings can borrow the exact CF object through `native_value`. JSON descriptions are diagnostic, not a promise of lossless native serialization. Reference and opaque-value retention currently lasts for the provider session. Long-running reference retirement and event-driven invalidation are pending.

Native AX calls use a two-second per-element messaging timeout. There is no overall operation deadline or worker isolation yet. See [backend tasks](docs/backends.md) for the remaining platform work. C ABI and Node bindings are planned but are not exposed in this initial version.

## Validation

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
mkdir -p native/macos/.build
xcrun swiftc native/macos/fixture.swift -o native/macos/.build/fixture -framework AppKit
open -a Calculator
python3 tests/macos_e2e.py
```

The native test changes Calculator's expression, briefly opens a disposable fixture, and moves the pointer. It exercises semantic setters, Unicode input, and pointer delivery. It reports known process-directed pointer limitations separately from successful global-route tests. See [observed results](docs/validation.md) and the [session protocol](docs/session.md).
