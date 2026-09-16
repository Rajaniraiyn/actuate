---
title: Language API design
description: Review Rust, TypeScript, and Python API conventions for typed requests, session lifetimes, cancellation, and errors without changing native semantics.
---

Status: proposal for SDK adapters. The Rust provider traits exist today;
TypeScript and Python packages do not.

## Compose capabilities

Keep observation, semantic actions, pointer input, capture, and cursor rendering
as separate contracts. A provider contributes the operations it implements.
Composing a renderer must not add input methods, and composing a capture provider
must not imply that it can inspect accessibility elements.

Use provider-specific types when the provider is known at compile time. A provider
loaded by name needs runtime capability checks before returning a supported
operation handle. Permissions, disconnected devices, and stale windows remain
runtime conditions even when the method exists in the type.

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
delivery and action variants. Infer result types from the provider and options
instead of requiring callers to repeat generic parameters.

An `observeOptions` helper should preserve inference when callers extract and
reuse a request. This follows TanStack's
[query options pattern](https://tanstack.com/query/latest/docs/framework/react/guides/query-options).
It should build data, not execute a request or create a global session.

Branded element, frame, and window references can prevent accidental interchange.
They cannot prove that two values belong to the same live session. Validate the
session identity and epoch at the native boundary too.

Proposed lifecycle, not executable SDK code:

```typescript
const session = await Actuate.connect({ provider: "native" });
try {
  const snapshot = await session.observe({ pid, maxNodes: 100 });
  const element = snapshot.root;
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
Use `Protocol` for structural provider contracts and generics for typed results.
Keep native attributes as explicitly unknown values where providers differ.

Use immutable request objects for composition. Validate external data at the
transport boundary, rather than repeatedly converting typed in-process values
through dictionaries. Vercel's Python SDK separates executor protocols, typed
requests, and asynchronous stream ownership in its
[model API](https://github.com/vercel-labs/ai-python/blob/142b074550b81b230e4c8077bf8a32527319791c/src/ai/models/core/api.py).

Proposed lifecycle, not executable SDK code:

```python
with Actuate.connect(provider="native") as session:
    snapshot = session.observe(pid=pid, max_nodes=100)
    element = snapshot.root
    details = session.inspect(element)
```

Provide an asynchronous client with `async with` and awaitable methods where
needed. Do not hide an event loop inside synchronous methods. Typed exceptions
should retain the native error code and effect.

## Extensions and middleware

Register extensions on a session or provider registry, with a unique name,
contract version, capability description, and request and result schemas. Infer
extension methods from the registered map. Unknown plugin data must remain
`unknown` until validated; importing a type declaration must not install behavior.

TanStack Table's
[feature maps](https://github.com/TanStack/table/blob/21d713fc4947d2a08cc2136bb055889a61412ded/packages/table-core/src/types/TableFeatures.ts)
show how selected features can contribute methods and options. Actuate should
scope those contributions to the configured instance rather than make every
extension appear on every session.

Provider contracts need explicit versions at an external plugin boundary.
Vercel AI SDK's
[provider contract](https://github.com/vercel/ai/blob/c97fcad34df41cd6d589375881f200154b8e5599/packages/provider/src/provider/v4/provider-v4.ts)
and [typed registry](https://github.com/vercel/ai/blob/c97fcad34df41cd6d589375881f200154b8e5599/packages/ai/src/registry/provider-registry.ts)
are references for this separation. They do not imply a stable native Rust ABI.

Middleware should wrap one operation contract with a documented order. For
`[trace, policy]`, trace enters first, policy enters second, then the provider
runs; results unwind in reverse. Keep callbacks for telemetry separate from
middleware that can reject or transform requests.

After a transformation, validate the request again. Middleware must not erase an
unknown effect, change window delivery to global delivery, or retry a mutation
automatically. Provider replacement invalidates its references and cached frames.
An external plugin crash must release owned resources and report uncertain
in-flight effects.

See the [reference review](https://github.com/Rajaniraiyn/actuate/blob/docs/_specs/sdk-reference-audit.md)
for inspected sources and implementation gaps.

## Cross-language checks

Use shared protocol fixtures to verify target scope, invalid coordinates, stale
references, unsupported routes, partial observations, and unknown effects.
Serialization tests alone do not establish native behavior. Run provider tests
separately on the relevant OS.

Add compile-time cases for inferred result types, missing capabilities,
extension-map inference, incompatible coordinates, and exhaustive action unions.
Test runtime rejection of foreign sessions, malformed plugin messages, and
middleware scope changes. Test cleanup after cancellation and plugin failure.
