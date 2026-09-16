---
title: CLI design
description: Review proposed action commands, named-session ownership, and output conventions. Distinguish this design from the current JSONL session interface.
---

Status: proposal. The current CLI performs actions through `session --json`.

## Commands follow the task

Keep discovery, observation, and action commands short. Put provider and session
selection in global options. Use separate verbs for replacing text and typing
input so callers do not need to infer the effect.

Proposed syntax:

```sh
actuate --session work snapshot --app 1234
actuate --session work inspect @e7
actuate --session work invoke @e7
actuate --session work fill @e9 "report.txt"
actuate --session work capture report.png --window 42
```

These commands require a session service that does not exist yet. A named session
must have a defined owner, explicit close behavior, transport permissions, and
reference lifetime before standalone action commands can ship.

## Target and delivery

`invoke` should choose an advertised default semantic action or return an
unsupported error. It must not silently click the global pointer. Pointer and
keyboard commands should require a delivery mode when scope is ambiguous.

`fill` replaces an editable value. `type` delivers text through the chosen input
route. A drag needs source and destination coordinate spaces, a motion policy,
and cleanup behavior if interrupted.

## Output and errors

Keep text readable when piped. JSON should carry the same result and effect
information without parsing prose. Send diagnostics to stderr and results to
stdout. Define stable exit-code categories before clients depend on them.

Generate help and the command specification from the parser. Test examples
against the compiled CLI to catch drift. A command must not report success solely
because an event entered an OS queue.
