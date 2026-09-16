---
title: Extend Actuate
description: Implement native providers with independent Rust capability traits. Compose observation, input, capture, and cursor rendering in your application.
---

The `actuate` crate contains the shared request types, provider traits, session
framing, and output rendering. Platform crates implement the native operations.

## Implement a capability

Implement the traits your provider supports. `ObserveScope` has an associated
`Scope` type, so a provider can use an application, window, or device-specific
target without converting every target to a process ID.

| Trait | Responsibility |
| --- | --- |
| `ObserveScope` | Return a bounded snapshot for the provider's scope. |
| `SemanticActions` | Perform an operation advertised by an element. |
| `Capture` | Capture an image with its coordinate mapping. |
| `CursorVisualization` | Render a cursor independently of input delivery. |
| `TouchInput` | Deliver a touch action in normalized input coordinates. |
| `HidKeyboard` | Deliver keyboard actions through the provider's HID route. |
| `AppLifecycle` | Manage applications supported by the provider. |

Providers own native handles. Public element references identify those handles
within a session; callers cannot transfer ownership by copying a reference.
Return `NativeError` with an effect that describes whether an operation was sent.

## Compose a backend

Start with `Backend::new(observation)`. Add providers through `with_input()`,
`with_capture()`, `with_cursor()`, and `with_apps()`. Each method returns a backend
whose type includes the selected provider. Trait bounds make methods available
only when the corresponding implementation exists.

A cursor renderer does not supply input, and a capture provider does not supply
accessibility observation. See the [Rust guide](/build/rust) for an example.

## Build a transport adapter

Use the [JSONL session protocol](/automate/sessions) when your application runs
outside Rust. Keep one process open for operations that share references. Forward
structured errors and receipt effects to the caller, and keep diagnostics on
stderr so stdout remains a stream of JSON replies.

Use the core's presentation module for text and compact output. Observation
budgets and display limits are separate; a formatted view must retain information
about incomplete traversal and omitted data.

## Contribute

The repository's [technical specifications](https://github.com/Rajaniraiyn/actuate/blob/main/_specs/README.md)
contain protocol details, proposed interfaces, and provider validation instructions.
The [SDK design](https://github.com/Rajaniraiyn/actuate/blob/main/_specs/language-api-design.md)
covers extension registration, middleware, and language conventions.
