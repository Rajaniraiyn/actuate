---
title: Targeting and references
description: Distinguish application identity, session-owned element references, and coordinate spaces. Choose semantic actions or an explicit input route.
---

Discovery identifies applications and windows. Observation produces element
references owned by a session. Inspection reports native attributes and actions.

Do not treat a matching label as proof of identity. Dialogs, menus, and virtualized
lists may replace native elements while keeping the same text. Observe again
after a structural change and resolve stale-reference errors explicitly.

## Semantic and pointer actions

A semantic action asks a native control to perform an advertised operation.
It can still change focus. A pointer action needs a valid coordinate space and
delivery route. Global input uses the shared desktop and may interrupt the user.

Window bounds, screenshot pixels, and desktop coordinates are different units.
Use frame mappings and explicit conversions. A point from an old screenshot may
be stale after a window moves or a display changes scale.

See [snapshots](/guides/snapshots) for partial observations and
[input and cursors](/guides/input-and-cursors) for visualization scope.
