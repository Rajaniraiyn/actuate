---
title: Errors and effects
description: Interpret native error codes and effect states, handle stale references, and decide when observation is needed before another action.
---

Session errors contain `code`, `message`, and `effect`. Use the code for program
logic and the message for diagnostics. Preserve the native code when wrapping an
error in an SDK or agent tool.

| Effect | Meaning | Next step |
| --- | --- | --- |
| `none` | No native side effect is reported. | Correct the request or resolve the missing capability. |
| `dispatched` | Input or an operation was sent. | Observe the target to verify the result. |
| `unknown` | The operation may have changed native state. | Inspect current state before deciding whether to retry. |

## References and capabilities

A stale reference requires a new observation. Resolve the intended target again
using its identity and context. Do not substitute a nearby coordinate silently.

An unsupported capability requires another explicitly selected route or a change
to the task. A permission failure requires the relevant OS permission; repeating
the same request does not grant it.

## Timeouts and cancellation

Stopping a wait does not prove that a native call stopped. A process can lose its
transport after dispatch, and an app can consume an event after the caller's
deadline. Treat those outcomes according to their reported effect.

Read-only polling can have a bounded retry policy. Input actions should not
inherit automatic retries from an HTTP client or a generic task runner.

See [sessions](/automate/sessions) for framing and
[targeting](/automate/targeting) for identity and coordinate rules.
