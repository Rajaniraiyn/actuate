---
title: TypeScript
description: Use typed requests and asynchronous sessions to inspect native apps from TypeScript. Reuse observation options and handle native action errors.
---

> SDK preview: this guide defines the TypeScript binding API. The package is not
> implemented yet. Use the [session protocol](/automate/sessions) for integration today.

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
