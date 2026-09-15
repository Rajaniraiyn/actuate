# Session protocol

Run `unimation session --json` for the JSONL protocol. Without `--json`, the CLI returns a framed text transcript. Send one JSON object per line. Receive one response per line, in order. The process owns the reference namespace until EOF. An optional `id` of any JSON type is echoed on valid-JSON requests, including operation errors. Parse errors do not terminate the session.

The authoritative request types are `unimation::SessionRequest`, `SemanticAction`, `PointerAction`, and `Delivery`, plus `macos::session::MacRequest` for native extensions. Unknown fields are rejected before dispatch. Replies contain either `result` or a structured `error` with `code`, `message`, and `effect`.

| Operation | Fields | Result |
| --- | --- | --- |
| `discover` | Optional `scope: all/apps`, `format: json/text/compact` | Session, permission state, applications; defaults to all/native JSON |
| `observe` | `request: {pid, max_nodes?, max_depth?}` | Snapshot; budgets default to 1000 and 30 |
| `snapshot` | `request`, optional `scope`, `options`, `format` | Observe, retain raw state and render in one request |
| `observe_subtree` | `target`, optional `max_nodes` and `max_depth` | Snapshot rooted at a live reference |
| `view` | Optional `revision`, `options`, `format` | Render a retained observation |
| `diff_view` | `before`, optional `after`, `options`, `max_changes` | Bounded text diff of presentation fields |
| `actionability` | `target` | Live native states and route-specific evidence; no input |
| `inspect` | `target: {session, id}` | Attribute values and supported action names |
| `attribute` | `target`, `name` | One native attribute value |
| `semantic` | `target`, `action` | Dispatch receipt |
| `pointer` | `delivery`, `action` | Dispatch receipt |
| `text` | `delivery`, `text` | Dispatch receipt |
| `key` | `delivery`, `chord: {key_code, modifiers?}` | Native virtual-key receipt |
| `parameterized_attribute` | `target`, `name`, `parameter` | Native parameterized value |
| `wait_attribute` | `target`, `name`, raw `expected`, optional `timeout_ms` | Poll a native value with a deadline |
| `snapshots` | None | Retained revision/root/coverage metadata |
| `query` | `query`, optional `revision` | Full matching and indeterminate nodes |
| `diff` | `before`, optional `after` | Observation differences |
| `hit_test` | Desktop `point` | Live AX reference |
| `window` | `target` | Native window and geometry |
| `click` | `target`, `mode`, optional `button`, `count`, `modifiers` | Explicit reference activation/input |
| `displays`, `windows`, `capabilities` | None | Native discovery/provider metadata |
| `capture` | `source`, `path`, optional `backend`, `max_pixel_edge` | Frame ID, file and coordinate mapping |
| `click_image` | `frame`, pixel `point`, `mode`, optional `button`, `count` | Input using a retained frame |
| `click_window` | `target`, window-local `point`, `mode`, optional `button`, `count` | Input using live window geometry |
| `scroll_target` | `target`, `mode`, `vertical`, `horizontal` | Scroll at a reference's center |
| `skylight_pointer` | `target`, `action` | Window-targeted native pointer sequence |
| `cursor_overlay` | `action: {kind: start, executable}` or `{kind: stop}` | Optional helper lifecycle, no render acknowledgement |
| `cursor_state` | None | Last SkyLight dispatch and visual-helper status |

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

Click defaults to one left-button down/up pair; `button` also accepts `right` and `middle`, and `count` supports 1 through 3. `modifiers` has optional `shift`, `control`, `alt`, and `meta` booleans. Scroll uses pixel deltas with an optional desktop `point`; omission uses the current pointer position. Drag accepts `from`, `to`, optional `button`, `modifiers`, and `duration_ms`. Delivery is `{"kind":"global"}` or `{"kind":"process","pid":1234}`. A process route does not select a window or focused control inside that process.

AX may advertise optional attributes that have no value. Their native `no_value` or `attribute_unsupported` results remain in the response without making the snapshot incomplete by themselves. Other failed reads, failed enumeration, and traversal limits set `complete:false`. `traversal_complete` separately reports whether the requested AXChildren traversal finished; attribute errors can coexist with complete traversal. Node-budget truncation includes `issues` with a pending-node count; depth truncation is recorded on its node. Completeness never means all UI appeared atomically or that hidden/provider-omitted content was discovered.

The implementation does not silently normalize native text, infer an action from a role, activate a target, or retry a mutation. An `unknown` effect means the caller must inspect the resulting state before deciding whether retry is safe. Route-specific native attribute names are intentional backend extensions.

## Snapshot queries and differences

Raw `observe` results remain available through `session --json`. Explicit `snapshot` and `view` requests can return text or compact JSON. `session --format compact` projects raw observation responses while retaining JSONL framing. `session --format text` changes the outer CLI response into a framed text transcript; use `--json` for existing machine clients.

```json
{"op":"snapshot","request":{"pid":1234,"max_nodes":1000,"max_depth":30},"scope":"focused_window","format":"text","options":{"actionable_only":true,"max_nodes":80}}
{"op":"view","revision":1,"format":"compact","options":{"hide_known_hidden":true,"max_nodes":80}}
{"op":"view","revision":1,"format":"text","options":{"root":{"session":"SESSION","id":12},"max_text_chars":120}}
{"op":"actionability","target":"@e12"}
{"op":"diff_view","before":1,"after":2,"options":{"max_text_chars":120},"max_changes":50}
```

Use actual returned IDs. `snapshot` collects a new observation and retains its full raw data. `view` only renders a retained observation. `format:"json"` returns raw data; `format:"compact"` returns presentation rows and coverage; `format:"text"` returns a tree string. `options.max_nodes` limits displayed rows, separately from `request.max_nodes`, which limits native collection. The presentation defaults are 200 displayed nodes and 160 characters per preview. Unknown visibility is retained by `hide_known_hidden`.

The new `snapshot` and `view` operations default to text when `format` is omitted. `snapshot.scope` defaults to `application`; request `focused_window` to resolve the focused AX window before collection. This is separate from the CLI `snapshot` alias of `observe`, which defaults to text.

`diff_view` compares projected fields and defaults to 100 emitted changes. It reports `entered_view`, `left_view`, omissions and uncertain coverage. It does not claim native destruction or equality of omitted attributes. The raw `diff` operation is unchanged and remains available for complete field-level evidence.

Short references such as `"@e12"` are expanded in the current live session. They do not become valid in another process. Native JSON references retain their full session and numeric ID. See [presentation and visibility](presentation.md) for viewport hints, adapter contracts and the difference between an interactive candidate and route-specific actionability.

```json
{"op":"query","query":{"role":{"kind":"exact","value":"AXButton"},"name":{"kind":"contains","value":"Equal"}}}
{"op":"query","revision":2,"query":{"attributes":{"AXEnabled":{"type":"bool","value":true}},"action":"AXPress"}}
{"op":"diff","before":1,"after":2}
```

Revision omission selects the latest retained observation. The session retains 32 snapshots. Different roots or sessions, duplicate references and non-increasing revisions are rejected by diff. `newly_observed` does not imply creation. `removed_from_scope` requires complete traversal and never implies destruction; incomplete absence appears as `no_longer_observed`. Modified fields preserve before/after values and presence flags, so native null remains different from an unobserved attribute. Read errors and opaque handle changes have uncertain evidence.

Role matches native `AXRole`. Name checks `AXTitle`, `AXDescription` and `AXLabel`; it does not invent one canonical name. Native attribute predicates compare full JSON values including type tags. Filters are combined with AND. Returned nodes keep all attributes, actions and issues. `indeterminate` preserves nodes whose requested predicate could not be established. Query and diff operate on saved observations and never refresh them implicitly.

## Screenshots and coordinate routing

```json
{"op":"capture","source":{"kind":"display","display_id":1},"path":"/tmp/unimation-frame.png","max_pixel_edge":1600}
{"op":"capture","source":{"kind":"window","window_id":42},"path":"/tmp/unimation-window.png","backend":"native"}
{"op":"click_image","frame":1,"point":{"x":300,"y":200},"mode":"global"}
{"op":"click_window","target":{"session":"SESSION","id":12},"point":{"x":80,"y":100},"mode":"skylight"}
```

Use discovered IDs and fresh output paths; example IDs are placeholders. Native ScreenCaptureKit is the default backend. `executable` explicitly selects `screencapture` and does not support `max_pixel_edge`. No provider falls back automatically.

A frame records desktop logical bounds, output pixel dimensions, owner when applicable and geometry revision. Pixel mapping accounts for downscaling and negative display origins. Session-owned frame IDs prevent client-supplied transforms. Changed geometry or a subsequent session mutation with dispatched/unknown effect invalidates image clicking. Invalid requests without effects do not invalidate frames. External UI changes can still occur without a geometry change.

Display images support global input. Window images support global, process and SkyLight input. Global window-image clicks additionally require the current AX hit-test window/owner to match the capture. Process mode selects a PID and retains its delivery limitations. SkyLight uses window-local coordinates. Semantic activation requires a reference and has no pixel meaning.

## Optional cursor visualization

```json
{"op":"cursor_overlay","action":{"kind":"start","executable":"/absolute/path/to/overlay"}}
{"op":"cursor_state"}
{"op":"cursor_overlay","action":{"kind":"stop"}}
```

The explicit helper path is resolved without a shell or PATH search. Startup leaves the overlay hidden. Subsequent successful SkyLight pointer dispatches queue a visualization at the last dispatched point and show the cursor. Moves animate for 180 ms; clicks move immediately and pulse. Visualization follows input dispatch, rather than delaying input until the animation arrives. It does not mirror the physical pointer or observe application acceptance.

`cursor_state` reports the last dispatched SkyLight pointer and visual-helper errors. Queue success does not acknowledge rendering. A visual failure never converts successful input into a retryable operation error. Inspect visual status separately and do not repeat input merely because the cursor was not visible. Stop or session teardown ends the helper. The helper also exposes its own [visual-only protocol](../crates/overlay/README.md).

Reference pointer clicks use an AX bounds center as a convenience heuristic. Bounds can span noninteractive space, including blank regions beside checkbox controls. A delivered center click may do nothing. Verify acceptance with a fresh observation, or select an explicit coordinate or an advertised semantic action appropriate to the task.

Reference pointer routes check the owning window's advertised `AXSheets` and direct `AXChildren` for attached sheets. An underlying reference returns `blocked_by_modal` before dispatch. Global image clicks also check the element hit at the mapped point. This is not a complete modal-window graph: separate app-modal windows, unadvertised overlays and raw coordinate/PID delivery still require caller observation and routing.

## System controls and attributed names

`{"op":"discover","scope":"apps","format":"text"}` returns a JSON string containing
an escaped, line-oriented application list. The default discovery request keeps every
record. Filtering is a presentation choice and does not invalidate references.

macOS attributed strings encode as `{"type":"attributed_string","text":...,"native":...}`.
`text` contains the decoded native string; `native` retains the original styled object.
Name queries and compact views use attributed titles, descriptions and labels when plain
names are unavailable. They never parse native debug descriptions into names.
Expected absent AX values are omitted from compact value previews. Other unavailable or
opaque values have a short inspect marker; raw attributes and coverage counts remain available.

Custom AX actions must use the exact string returned in `actions`, including embedded
newlines. Control Center's Wi-Fi details action is one such action. `AXShowMenu` can
open an editing menu instead of network details, even when the returned effect is unknown.
Reobserve before deciding whether to retry an action.

A focused AX window does not prove its process owns global keyboard input. On this host,
Control Center had a focused nonactivating panel while Ghostty remained frontmost.
Panel tests dismiss with process-directed Escape and verify that the panel disappeared.
They must not fall back to global Escape, which can interrupt the hosting terminal.

## iOS Simulator sessions

`unimation --provider ios --device UUID --device-set /absolute/path session` selects the iOS provider.
It uses a frontmost guest application rather than the macOS PID observation selector.
See [the iOS protocol](ios.md#jsonl-session) for supported operations and current limits.
