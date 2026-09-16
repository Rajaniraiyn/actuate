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

## Handle application and filesystem differences

A Save dialog may append a default extension or interpret a relative filename
against its current directory. Read back the resolved filename and check the
saved location. For overwrite prompts, choose the confirmation explicitly and
verify whether the original file changed.

Network folders and cloud placeholders can delay completion after a dialog
closes. Permission failures, locked files, and invalid names may produce another
dialog or an inline message. Observe the application before deciding to retry.

Custom file pickers may expose different controls from native dialogs. Inspect
the available actions and values instead of assuming a fixed control order.
For tab-to-tab transfers, select and observe the destination tab before resolving
its drop target. Refresh coordinates after scrolling or changing tabs.
