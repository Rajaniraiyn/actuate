# Session protocol

Run `unimation session`. Send one JSON object per line. Receive one response per line, in order. The process owns the reference namespace until EOF. An optional `id` of any JSON type is echoed on valid-JSON requests, including operation errors. Parse errors do not terminate the session.

The authoritative request types are `unimation_core::SessionRequest`, `SemanticAction`, `PointerAction`, and `Delivery`. Unknown fields are rejected before dispatch. Replies contain either `result` or a structured `error` with `code`, `message`, and `effect`.

| Operation | Fields | Result |
| --- | --- | --- |
| `discover` | None | Session, permission state, applications |
| `observe` | `request: {pid, max_nodes?, max_depth?}` | Snapshot; budgets default to 1000 and 30 |
| `observe_subtree` | `target`, optional `max_nodes` and `max_depth` | Snapshot rooted at a live reference |
| `inspect` | `target: {session, id}` | Attribute values and supported action names |
| `attribute` | `target`, `name` | One native attribute value |
| `semantic` | `target`, `action` | Dispatch receipt |
| `pointer` | `delivery`, `action` | Dispatch receipt |
| `text` | `delivery`, `text` | Dispatch receipt |

Semantic action shapes:

```json
{"kind":"perform","name":"AXPress"}
{"kind":"set_string","attribute":"AXValue","value":"hello"}
{"kind":"set_bool","attribute":"AXFocused","value":true}
```

Pointer action shapes:

```json
{"kind":"move","point":{"x":100.0,"y":200.0}}
{"kind":"click","point":{"x":100.0,"y":200.0}}
{"kind":"scroll","vertical":20,"horizontal":0}
```

Click currently means one left-button down/up pair. Scroll currently uses pixel deltas at the current pointer location. Delivery is `{"kind":"global"}` or `{"kind":"process","pid":1234}`. A process route does not select a window or focused control inside that process.

AX may advertise optional attributes that have no value. Their native `no_value` or `attribute_unsupported` results remain in the response without making the snapshot incomplete by themselves. Other failed reads, failed enumeration, and traversal limits set `complete:false`. `traversal_complete` separately reports whether the requested AXChildren traversal finished; attribute errors can coexist with complete traversal. Node-budget truncation includes `issues` with a pending-node count; depth truncation is recorded on its node. Completeness never means all UI appeared atomically or that hidden/provider-omitted content was discovered.

The implementation does not silently normalize native text, infer an action from a role, activate a target, or retry a mutation. An `unknown` effect means the caller must inspect the resulting state before deciding whether retry is safe. Route-specific native attribute names are intentional backend extensions.
