# Plan 23: Tombstone GC (decision + eventual design)

Deliberately deferred — this plan exists so the deferral is a tracked decision, not a blind
spot. Requires plan 11 item 1 (real `deleted_at` timestamps) first, or ages are meaningless.

## Problem

`book_dir` and `dir` tombstones accumulate forever. They are **load-bearing**: a tombstone's
HLC is what makes a later lower-HLC add lose the LWW gate (`apply_remote_book_dir_removed`'s
comment). Deleting one too early re-opens the resurrection race it exists to prevent.

## When it matters

Personal-library scale: years away. Trigger condition to revisit: tombstone count visibly
affecting DB size or scan/query time (add a debug count to plan 22's status if curious).

## Sketch of the eventual design (do not build yet)

A tombstone is safely deletable when no event that could lose to it can still arrive. With a
single authoritative server: safe once every registered device's pull cursor has passed the
tombstone's event — i.e. GC needs **device liveness tracking** server-side (last-seen cursor
per client_id) and a policy for abandoned devices (e.g. a device silent for >N months is
dropped from the constraint; if it ever returns, it must re-bootstrap from a snapshot rather
than incremental pull). That makes tombstone GC a sibling of append-log compaction (plan 24)
and snapshot bootstrap — design them together when the trigger fires.

## Acceptance (for now)

This file exists; plan 11's timestamp fix is done; a one-line note in plan 24 cross-links.
