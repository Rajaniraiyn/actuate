# Compact output and visibility

Actuate retains the native observation and derives a smaller view for agents. Raw attributes, errors and session-qualified references remain available. Text and compact JSON are presentation formats; neither replaces the stored snapshot.

## Choose an output format

```sh
actuate observe 1234 --scope window --format text --interactive --limit 80
actuate observe 1234 --format compact --limit 80
actuate view snapshot.json --root @e12 --interactive --limit 80
actuate diff before.json after.json --format text
```

Replace the PID and reference with discovered values. `observe`, `session`, and `view` default to text. Use `--json` for full machine output. `--format compact` emits JSON containing presentation rows and coverage metadata. `--format json` exposes the native snapshot.

`--interactive` includes advertised actions and control candidates; it does not establish that a click will work. `--hide-hidden` excludes known hidden nodes and keeps unknown visibility. `--limit` limits displayed rows. The native `--max-nodes` and `--max-depth` collection budgets still control how much of the tree is read.

Names and values have bounded previews with omitted-character counts. The stored text is unchanged. Short `@eN` references mean the same session-qualified elements as raw JSON, not a new numbering for each display. A saved snapshot can be rendered after its session ends, but its references cannot be used to act in a new session.

## Scope before filtering

An application's accessibility root can contain hundreds of menu commands alongside its windows. For a task inside an editor or website, first find the relevant window or web area, then observe that subtree. Rendering a narrower root from an existing snapshot is useful, but cannot recover children missing from the original collection budget.

`--scope window` resolves the application's focused AX window before traversing it. The default `--scope application` keeps the original application traversal. For a more specific web area or scroll container, use `observe_subtree` with a live reference.

Keep content-only views available when reading. An interactive-only view can omit the error message, result text or status that establishes whether an action succeeded. Use the raw snapshot, a less restrictive view or a targeted attribute read when the compact view lacks that evidence.

## Visibility and actionability

| Evidence | What it establishes | What it does not establish |
| --- | --- | --- |
| Native hidden/visible state | An attribute reported by the provider | Complete desktop visibility |
| Bounds intersect a viewport | Geometric overlap in a declared coordinate system | Ancestor clipping or unobscured pixels |
| Enabled state | Provider-reported enablement | Successful action consumption |
| Global center hit test | The sampled point reaches the element or its descendant | Every point is reachable, or a later click is safe |
| Attached-sheet check | A supported check found or did not find a sheet blocker | A complete graph of all modal windows |
| Advertised semantic actions | The element lists an action | That the application will accept it now |

Missing information remains unknown. A visible control can be covered; a covered window can accept a targeted SkyLight event; an offscreen element can expose a semantic action. Consequently there is no universal `clickable` boolean.

The `actionability` request performs live reads and checks without input, scrolling or activation. Its `routes` distinguish global hit testing, untested SkyLight delivery and advertised semantic actions. Ancestor clipping remains unknown in the current implementation. Reads occur sequentially, so even this live report is not an atomic desktop state.

Compact rows carry `viewport_relation` as `above`, `below`, `left`, `right`, `inside`, `partially_inside` or `unknown`. The AX adapter selects the nearest observed `AXScrollArea` or `AXWindow` ancestor and records its reference. It separately records the nearest scroll-container reference. Missing bounds or ambiguous ancestry leave geometry unknown. An `AXWebArea` bound may span document content, so the default AX adapter does not treat it as a viewport. A browser adapter can supply actual viewport geometry. These relations remain layout hints rather than browser viewport guarantees.

Reference pointer routes reject explicit native hidden, invisible or minimized state in the target ancestry, and a target explicitly reported disabled. Reference-center clicks also reject a point outside known native AXScrollArea or AXWindow ancestor bounds with `outside_viewport`; scroll and observe again. The compact projection inherits known hidden state only through unambiguous observed ancestry and records the responsible ancestor. These checks do not change the contract of raw coordinate input or semantic actions. Missing native flags remain unknown; a point hit test still cannot prove future application consumption.

## Scroll, observe and compare

Use the existing session operations as a bounded loop:

1. Observe the intended window or scroll-container subtree and retain its revision.
2. Render the relevant content and select a current target.
3. Send `scroll_target` using an explicit route.
4. Observe the same subtree again.
5. Compare revisions and inspect content, values or geometry that demonstrate progress.

A dispatch receipt does not establish movement. Repeated content can result from virtualization or overlapping pages. An unchanged compact view can also conceal native changes outside its displayed fields. These observations are not sufficient to declare the end of a list. Automatic scroll-until orchestration is not implemented by the presentation layer.

Raw diffs retain before/after values and distinguish newly observed nodes, complete-scope removals and uncertain absence. Display filtering must never turn a node leaving the viewport into a destruction claim. Use the same scope and presentation settings when comparing views. Always retain the raw diff when omitted attributes matter to the task.

The library's `render_view_diff` compares compact fields and reports `entered_view`, `left_view` and modified fields. It limits emitted changes and reports omissions and uncertainty. It suppresses opaque-value handle churn; unchanged truncated previews do not establish equality of complete native values.

Use the session `diff_view` operation to configure presentation options and a change limit. CLI `diff --format text` uses the default presentation and a 100-change limit. `diff --format compact` wraps that bounded text with root and revision metadata in JSON; it cannot be combined with the native-only `--modified-only` filter. Combining `--modified-only --format text` retains the native modified-field renderer, useful when the omitted native details are the subject of the comparison.

## Library boundaries

Native providers own observation and reference resolution. The independent `Actionability` trait exposes a backend-specific report without requiring one global boolean. Presentation adapters interpret native attributes; generic rendering owns traversal of retained data, output budgets and text formatting. An adapter's role or name mapping does not rewrite the original node. Viewport geometry is additional evidence, not permission to dispatch an action.

Rust callers can supply `PresentationAdapter` to `render_snapshot_with_adapter` and `render_view_diff_with_adapter`, including a runtime `dyn PresentationAdapter`. The supplied `AxPresentationAdapter` implements AX-specific attribute and geometry interpretation; `render_snapshot` is its convenience entry point. All bounds supplied by an adapter must use one common top-left coordinate space within the snapshot. The renderer owns reference identity even when an adapter supplies display fields.

Unknown provider attributes, sparse trees, inaccessible embeddings and collection truncation must remain observable through coverage metadata or raw inspection. A small display is not evidence of a complete UI.

## Reference implementations

- Agent-browser documents independent interactive, compact, depth and scoped snapshot controls, and a default text tree with references. Actuate adopts the presentation pattern with opt-in full JSON output. [Snapshot documentation](https://agent-browser.dev/snapshots), [pinned renderer](https://github.com/vercel-labs/agent-browser/blob/8bbddb840c74d3c41b01d0b6804b059ba40de56e/cli/src/native/snapshot.rs#L1083).
- Agent-browser's snapshot diff compares rendered lines. Actuate retains its semantic diff beneath presentation so truncation and uncertain absence retain their meaning. [Diff documentation](https://agent-browser.dev/diffing), [pinned diff implementation](https://github.com/vercel-labs/agent-browser/blob/8bbddb840c74d3c41b01d0b6804b059ba40de56e/cli/src/native/diff.rs).
- Browser-use collects layout, scroll rectangles and paint order in its enhanced snapshot processing. Those inputs suggest separate evidence fields for a future browser adapter rather than one generic visibility guess. [Pinned enhanced snapshot code](https://github.com/browser-use/browser-use/blob/843819cb8131e1370948d381ede9be7f8366ddc4/browser_use/dom/enhanced_snapshot.py), [CDP DOMSnapshot protocol](https://chromedevtools.github.io/devtools-protocol/tot/DOMSnapshot/).
- Playwright separates visibility, stability, enablement, editability and receiving events. Its visible-state definition is not a viewport-intersection check. Actuate keeps those distinctions across native input routes. [Actionability documentation](https://playwright.dev/docs/actionability).

These references inform design; they do not establish that a particular Actuate backend or application has passed a live test. See [validation results](validation.md) for tests actually run.
