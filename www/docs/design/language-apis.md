---
title: Language API design
description: Review Rust, TypeScript, and Python API conventions for typed requests, session lifetimes, cancellation, and errors without changing native semantics.
---

Status: proposal for SDK adapters. The Rust provider traits exist today;
TypeScript and Python packages do not.

## Rust

Keep native operations behind capability traits with associated request and
response types. Use `Result<T, NativeError>`, enums for delivery and actions, and
owned session references. Builders compose providers without making unsupported
methods appear available.

Do not force asynchronous methods around thread-bound native APIs. Expose an
asynchronous worker adapter separately when blocking or thread affinity requires
it. Dropping a future must not claim to cancel a native action already dispatched.

## TypeScript

Use `camelCase`, promises for transport operations, and discriminated unions for
delivery and action variants. Branded reference types should prevent accidental
mixing with arbitrary strings. Validate them again at the native boundary.

Proposed lifecycle, not executable SDK code:

```typescript
const session = await Actuate.connect({ provider: "native" });
try {
  const snapshot = await session.observe({ pid, maxNodes: 100 });
  const element = snapshot.roots[0];
  const details = await session.inspect(element);
} finally {
  await session.close();
}
```

Accept `AbortSignal` for waits and transport operations. Cancellation after
dispatch may leave an unknown effect; the rejected operation must expose that
state. Errors should carry `code`, `effect`, and their original cause.

## Python

Use `snake_case`, keyword-only options, type annotations, and context managers.
Represent requests with typed models rather than free-form dictionaries where
the schema is stable. Preserve native attributes where providers differ.

Proposed lifecycle, not executable SDK code:

```python
with Actuate.connect(provider="native") as session:
    snapshot = session.observe(pid=pid, max_nodes=100)
    element = snapshot.roots[0]
    details = session.inspect(element)
```

Provide an asynchronous client with `async with` and awaitable methods where
needed. Do not hide an event loop inside synchronous methods. Typed exceptions
should retain the native error code and effect.

## Cross-language checks

Use shared protocol fixtures to verify target scope, invalid coordinates, stale
references, unsupported routes, partial observations, and unknown effects.
Serialization tests alone do not establish native behavior. Run provider tests
separately on the relevant OS.
