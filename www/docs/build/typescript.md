---
title: TypeScript
description: Use typed requests and asynchronous sessions to inspect native apps from TypeScript. Reuse observation options and handle native action errors.
---

## Install

Download the TypeScript package archive from
[GitHub Releases](https://github.com/Rajaniraiyn/actuate/releases), then install it
with Bun:

```sh
bun add ./actuate-0.1.0.tgz
bun pm trust actuate
```

Use the filename from your selected release. The install hook downloads that
version's native addon and verifies its checksum. If lifecycle scripts are
disabled, run `bun run --cwd node_modules/actuate install-native` explicitly.
Set `GH_TOKEN` when accessing a private release.

## Open a session

A session owns the connection to a provider and the element references it returns.
Use `Actuate.connect()` to open it and `close()` to release it.

```typescript
import { Actuate } from "actuate";

const session = await Actuate.connect({ provider: "native" });
try {
  const apps = await session.discover();
  console.log(apps);
} finally {
  await session.close();
}
```

`native` selects the host platform. Call `session.capabilities()` to check the
operations and delivery routes available on that host.

## Observe an application

Pass a process ID from discovery. Observation returns a typed snapshot with a
root reference, nodes, completeness flags, and native read issues.

```typescript
const snapshot = await session.observe({
  pid,
  maxNodes: 500,
  maxDepth: 20,
});

const root = await session.inspect(snapshot.root);
console.log(root.attributes, root.actions);
```

Here and below, `session` is an open session and `pid` is the chosen process ID.
Check `snapshot.complete` before treating a missing node as absent. Element
references remain valid only within their owning session and native lifetime.

## Reuse options

`observeOptions()` preserves request types when options are shared between calls.
It creates an options object without opening a connection or performing an action.

```typescript
import { observeOptions } from "actuate";

const options = observeOptions({ pid, maxNodes: 500, maxDepth: 20 });
const before = await session.observe(options);
const after = await session.observe(options);
```

## Perform a native action

Inspect the target before selecting an action. Native names differ by platform.
This example uses the `invoke` action advertised by a Windows UIA control:

```typescript
const node = await session.inspect(target);
if (node.actions.includes("invoke")) {
  const receipt = await session.semantic(target, {
    kind: "perform",
    name: "invoke",
  });
  console.log(receipt.effect);
}
```

`target` is a reference from the session's observation. Action variants form a
discriminated union keyed by `kind`. Native attribute values require narrowing
before use because their types depend on the provider.

## Handle errors

`NativeError` retains the provider's `code`, `message`, and `effect`. Branch on
`code` for recovery and inspect the target when `effect` is `dispatched` or
`unknown`. An input receipt reports delivery; observe again to verify the change.

See [errors and effects](/guides/errors) for retry behavior and
[targeting](/automate/targeting) for reference and coordinate rules.

## Platform operations and extensions

`session.request()` exposes the selected provider's full session protocol,
including platform-specific shell, window, device, and cursor operations. It
returns `Json`; narrow the result before reading provider-specific fields.

```typescript
const state = await session.request({ op: "taskbar_state" });
```

This operation is Windows-specific. Unsupported operations retain the provider's
structured error. Use `defineOperation({ request, parse })` with `session.run()`
to add a typed operation whose input and result types are inferred from the two
functions. `Actuate.fromTransport(transport, { middleware })` accepts a custom
transport. Middleware runs in array order and results unwind in reverse order.

Calls within a session execute in invocation order. `close()` waits for prior
requests and rejects new ones. Native result fields retain their protocol names,
including `traversal_complete` and `parameterized_attributes`.
