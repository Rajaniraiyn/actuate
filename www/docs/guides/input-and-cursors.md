---
title: Input and cursors
description: Choose semantic actions or explicit pointer delivery, then configure cursor visualization independently of input and physical cursor visibility.
---

## Choose a delivery route

| Route | Use | Check first |
| --- | --- | --- |
| Semantic action | Invoke an operation exposed by an accessibility element. | The element advertises the action. It may activate the target. |
| Window-directed input | Send input through a provider's window route. | The route supports the target and coordinate mapping. |
| Global input | Use the shared keyboard and pointer. | The intended window is ready to receive input. |

Unsupported window-directed input must fail rather than switch to global input.
The current Windows input provider uses global SendInput; UIA actions are a
separate route. Check [platform coverage](/platforms) before choosing a route.

## Show a soft cursor

The optional `actuate-overlay` binary renders the soft cursor. A session starts
it through `cursor_overlay` with an explicit executable path. `cursor` commands
control its appearance and motion where supported.

The renderer can use window or display scope. Window scope follows the chosen
window and applies its visibility rules. Display scope is useful for shell
controls that are not attached to an ordinary application window.

## Physical cursor policies

Windows offers `preserve`, `hide_within_scope`, and `hide_while_visible` policies.
`preserve` is the default. `physical_pointer` tracking follows the real pointer;
command tracking follows positions supplied to the renderer.

Cursor hiding and input delivery are separate settings. A hidden system pointer
still belongs to the shared desktop. Moving the soft cursor alone does not
activate a control or reveal an auto-hidden taskbar.

## Motion and results

Cursor motion, idle animation, and click ripples share settings across renderers.
Reduced motion suppresses ornamental movement. A queued visual update is not a
render acknowledgement or proof that the app consumed input.

See the [overlay specification](https://github.com/Rajaniraiyn/actuate/blob/docs/_specs/windows-overlay.md)
for Windows command shapes and the [error guide](/guides/errors) for input receipts.
