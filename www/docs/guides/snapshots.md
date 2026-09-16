---
title: Snapshots
description: Read bounded accessibility trees, distinguish missing data from removed elements, and compare observations without changing native references.
---

A snapshot records the provider's accessibility tree, native attributes, and
element references. A presentation view filters that record for display. It does
not replace the underlying observation.

## Bound observation and output separately

```sh
actuate snapshot 1234 --max-nodes 500 --max-depth 20 --interactive --limit 100
```

`--max-nodes` and `--max-depth` limit traversal. `--interactive` and `--limit`
limit what the view shows. Interactive filtering depends on the native actions
and attributes the provider can read.

## Keep uncertainty

Check `complete`, `traversal_complete`, and reported issues. A native read may fail
or a traversal budget may end before the tree does. An element missing from an
incomplete snapshot is not necessarily gone.

Preserve native role and attribute names. UIA, AX, and AT-SPI do not expose
identical information. Querying by a display label alone can match several nodes.

## Compare changes

Save observations from the same session and compare their revisions. A diff
should retain unknown values and incomplete coverage. Use a new observation to
verify a value change or dialog transition, rather than inferring success from
the disappearance of a partially observed element.

See [commands](/automate/commands) for saved-snapshot tools and
[sessions](/automate/sessions) for reference ownership.
