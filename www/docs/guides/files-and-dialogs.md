---
title: Files and dialogs
description: Automate native Save and Open dialogs and file transfers with scoped observation, explicit confirmation handling, and checks of the final result.
---

## Save or open a file

Observe the dialog's owning process and window. A dialog can be a separate
top-level window, so an old application snapshot may not contain it. On Windows,
`observe` accepts an optional top-level `window_id` and checks its owning PID.

1. Inspect the filename edit control and its supported value operation.
2. Set the filename or path. Distinguish the edit control from a similarly named combo box.
3. Invoke the dialog's advertised Save or Open action.
4. Observe again for an overwrite confirmation, validation error, or completion.
5. Verify the resulting file or opened content through the application or filesystem.

Confirmation dialogs need fresh references. Do not reuse a Save button reference
after its dialog closes. Declining overwrite and cancelling should leave the
original file unchanged.

## Drag files

Select the item and choose a visible point inside its name or icon. A large row's
bounds may extend into empty space or a details pane. Starting there can draw a
selection rectangle instead of picking up the file.

Check the source and destination windows immediately before dragging. Use the
native drag sequence, then verify both locations and the transferred contents.
Do not infer copy versus move from the animation or a modifier alone.

## Tested cases

Windows fixtures cover Unicode paths, nested folders, default extensions,
overwrite acceptance and rejection, cancellation, invalid filenames, missing
files, and stale dialog references. Live Explorer tests verified file and folder
moves between two windows.

Tab-to-tab transfers, network paths, cloud placeholders, access-denied paths,
locked files, and elevated or custom dialogs still need validation. The
[Windows report](https://github.com/Rajaniraiyn/actuate/blob/docs/_specs/windows-interactive-results.md)
contains the evidence and test commands.
