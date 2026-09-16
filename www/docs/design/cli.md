---
title: CLI design
description: Review proposed action commands, named-session ownership, and output conventions. Distinguish this design from the current JSONL session interface.
---

Status: proposal. The current CLI performs actions through `session --json`.

The [agent-browser command reference](https://agent-browser.dev/commands) provides
the main reference for short task verbs and reference-based interactions. Its
[snapshot guide](https://agent-browser.dev/snapshots) shows how filtering and
bounded text keep observations useful for agents.

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

## Keep one output model

Render text, compact JSON, and full JSON from the same result. Keep renderer code
outside native providers so an SDK, CLI, or MCP adapter cannot change action
semantics by choosing an output format.

| Output | Contract |
| --- | --- |
| Text | Bounded trees and records with references and native labels. Escape control characters from app content. |
| Compact JSON | A presentation view with explicit omission and coverage information. |
| Full JSON | The result record and structured errors, including effect and observation issues. |
| Session JSONL | One complete reply per request line, with the caller's correlation ID. |

Truncation must be explicit and must not produce invalid JSON. Do not cut an
encoded object at a byte limit. Bound nodes or records before serialization.
Keep provider diagnostics off stdout. ANSI color and progress output must never
change the protocol.

Agent-browser centralizes rendering and bounded output in its
[output module](https://github.com/vercel-labs/agent-browser/blob/aff6125c023b810ea3f2e5deec5379e9a4270bdc/cli/src/output.rs).
Actuate already has shared presentation and transport modules. Extend those
instead of adding another renderer in each adapter.

## Plugin commands

Keep extension commands under an explicit namespace so they cannot shadow core
verbs. Derive help and validation from the extension's registered schema. An
external command adapter should use a versioned process protocol and report its
capabilities before dispatch.

Agent-browser's [plugin protocol](https://github.com/vercel-labs/agent-browser/blob/aff6125c023b810ea3f2e5deec5379e9a4270bdc/cli/src/plugins.rs)
is a useful reference for process separation. Actuate plugins must also preserve
native handle ownership and reject references from a previous provider process.
