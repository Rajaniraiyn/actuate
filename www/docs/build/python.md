---
title: Python
description: Inspect native apps with typed Python requests and context-managed sessions. Reuse observation options and preserve native errors and effects.
---

## Install

Download a compatible wheel or source package from
[GitHub Releases](https://github.com/Rajaniraiyn/actuate/releases):

```sh
uv venv
uv pip install ./actuate-0.1.0.tar.gz
```

Use the filename from your selected release. Python 3.10 or newer is required.
The source package downloads a matching prebuilt wheel during installation and
verifies its checksum. It does not need a Rust compiler. Set `GH_TOKEN` for private
release access. A downloaded wheel can also be installed directly with uv.

## Open a session

Use a context manager to own the provider connection. Leaving the block closes
the session and releases its element references.

```python
from actuate import Actuate

with Actuate.connect(provider="native") as session:
    apps = session.discover()
    print(apps)
```

`native` selects the host platform. Call `session.capabilities()` to check its
operations and delivery routes.

## Observe an application

Pass a process ID from discovery. Options are keyword-only, and results expose
typed fields for nodes, references, completeness, and native read issues.

```python
with Actuate.connect(provider="native") as session:
    snapshot = session.observe(pid=pid, max_nodes=500, max_depth=20)
    root = session.inspect(snapshot.root)
    print(root.attributes, root.actions)
```

`pid` is the chosen application's process ID. Check `snapshot.complete` before
treating a missing node as absent. Reobserve after a dialog closes or an
application replaces its controls.

## Reuse options

Use an immutable `ObserveOptions` value to share an observation budget across calls.

```python
from actuate import ObserveOptions

options = ObserveOptions(pid=pid, max_nodes=500, max_depth=20)
with Actuate.connect(provider="native") as session:
    before = session.observe(options=options)
    after = session.observe(options=options)
```

## Perform a native action

Inspect the target and select an advertised action. This example uses a Windows
UIA control's `invoke` action:

```python
from actuate import Perform

node = session.inspect(target)
if "invoke" in node.actions:
    receipt = session.semantic(target, Perform(name="invoke"))
    print(receipt.effect)
```

Run the action inside the session's context block. `target` is a reference from
that session's observation. Native names differ by platform; macOS controls may
advertise `AXPress` instead.

## Use an asynchronous session

`AsyncActuate` exposes awaitable operations and owns its connection through
`async with`. Use it within your application's event loop.

```python
from actuate import AsyncActuate

async def inspect_app(pid: int):
    async with AsyncActuate.connect(provider="native") as session:
        snapshot = await session.observe(pid=pid, max_nodes=500)
        return await session.inspect(snapshot.root)
```

## Handle errors

`NativeError` exposes `code`, `message`, and `effect`. Preserve those fields when
wrapping an exception. After an uncertain input result, observe the target before
repeating the action. Cancelling an await does not undo an action already sent.

See [errors and effects](/guides/errors) and
[targeting](/automate/targeting) for recovery and reference ownership.

## Platform operations and extensions

`session.request()` exposes the selected provider's full session protocol,
including platform-specific shell, window, device, and cursor operations.

```python
state = session.request({"op": "taskbar_state"})
```

This operation is Windows-specific. Other providers report their own supported
operations and structured errors. Use `Operation(request, parse)` with
`session.run()` to define a typed extension. `Actuate.from_transport()` accepts
an implementation of the `Transport` protocol and ordered middleware.

Native calls release the GIL and execute on the session's dedicated worker.
The asynchronous client offloads waiting from the event loop. Closing it waits
for in-flight work, including work whose caller stopped awaiting it.
