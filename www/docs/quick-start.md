---
title: Quick start
description: Discover an app, inspect its accessibility tree, and act through a supported native route. Keep references in one session and verify the result.
---

## Discover and inspect

```sh
actuate capabilities
actuate discover
actuate windows
actuate snapshot --help
```

Select an application's process ID from discovery. Replace `1234` below with
that ID:

```sh
actuate snapshot 1234 --interactive --limit 100
```

This command returns a bounded view of the app's accessibility tree. Native
roles, available actions, and completeness vary by provider.

## Keep a session open

Use one persistent process when later actions need references from a snapshot:

```sh
actuate session --json
```

Send one JSON request per line on stdin:

```json
{"id":"caps","op":"capabilities"}
{"id":"apps","op":"discover"}
{"id":"observe","op":"observe","request":{"pid":1234,"max_nodes":100,"max_depth":12}}
```

These requests return separate JSON replies. The `id` field associates a reply
with a request. Choose a reference from the observation and inspect it in the
same process. Replace `@e1` with an actual returned reference:

```json
{"op":"inspect","target":"@e1"}
```

## Act, then observe

Choose an action the element advertises. For example, a Windows UIA button may
support `invoke`:

```json
{"op":"semantic","target":"@e1","action":{"kind":"perform","name":"invoke"}}
```

This is a Windows example, not a portable action name. On macOS an AX button
may advertise `AXPress`; Linux reports its AT-SPI action names. Inspect first.

Observe again to verify the intended change. Do not retry an action merely
because its result is uncertain. Close stdin to end the session and release its
references. See [sessions](/automate/sessions) for framing and errors.
