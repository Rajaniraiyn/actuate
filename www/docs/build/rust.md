---
title: Rust
description: Compose native observation, input, capture, and cursor providers with Rust traits. Keep native handles and element references owned by their provider.
---

The `actuate` crate defines shared types and capability traits. Platform crates
implement them with native APIs. Add local path dependencies when embedding this
workspace in an application.

```toml
[dependencies]
actuate = { path = "../actuate/crates/actuate" }
```

## Compose providers

`Backend` takes an observation provider. Add input, capture, cursor visualization,
or application management only when you need them. Methods are available when
their trait bounds are satisfied.

This example uses the existing iOS simulator providers on macOS:

```rust
use actuate::{Backend, ObservationBudget, ObserveScope};

let mut backend = Backend::new(
    ios::SimulatorAccessibility::connect(udid, device_set)?
).with_input(ios::SimulatorHid::connect(udid, device_set)?);

let snapshot = backend.observe_scope(
    ios::SimulatorScope::Frontmost,
    ObservationBudget::default(),
)?;
```

`udid` identifies an existing booted simulator; `device_set` identifies its device
set. Add the `ios` crate as a path dependency for this example.

## Handle results

Native operations return `actuate::Result<T>`. `NativeError` contains a code,
message, and effect. Keep the effect when translating errors for another caller.
Providers own their native handles and enforce reference lifetimes.

Use typed requests and explicit coordinate conversions. Do not serialize calls
to JSON inside a Rust application when the provider already exposes a typed API.

See the [composition specification](https://github.com/Rajaniraiyn/actuate/blob/main/_specs/composition.md)
for available traits and [Extend Actuate](/developers) for provider and transport contracts.
