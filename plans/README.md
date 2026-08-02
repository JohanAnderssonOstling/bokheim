# Plan index

Each plan is self-contained: a fresh session can execute it without conversation history.
Update the Status column as work lands (✅ done · 🔶 partial · ⬜ open · 💤 deferred-by-design).

## Status overview

| # | Plan | Status | Depends on |
|---|------|--------|------------|
| 06 | [`book_meta` singleton + multi-author](06-book-meta-singleton.md) | ⬜ open | 01 |
| 07 | [Thumbnail keyspace + drop APP_DATA_DIR global](07-thumbnail-keyspace.md) | ⬜ open | — |
| 09 | [Scanner robustness](09-scanner-robustness.md) | ⬜ open | — |
| 10 | [Small cleanups (Backend)](10-small-cleanups.md) | ⬜ open | — |
| 11 | [DB small fixes (schema hygiene)](11-db-small-fixes.md) | ⬜ open | — |
| 12 | [Convergence harness, tier 1: metadata](12-convergence-harness-metadata.md) | ⬜ open | — |
| 13 | [Convergence harness, tier 2: disk + scanner](13-convergence-harness-disk.md) | ⬜ open | 12, 09, 14, 15 |
| 14 | [FIX: remote dir-delete ordering](14-fix-remote-dir-delete-ordering.md) | ⬜ open — **bug** | — |
| 15 | [FIX: orphan path truncation](15-fix-orphan-path-truncation.md) | ⬜ open — **bug** | — |
| 16 | [Pull pagination](16-pull-pagination.md) | ⬜ open | — (12 helps test) |
| 17 | [Server poke (SSE)](17-server-poke.md) | ⬜ open | — (plugs into 20) |
| 18 | [Resumable downloads](18-resumable-downloads.md) | ⬜ open | — |
| 19 | [Token expiry & 401 handling](19-token-expiry-handling.md) | ⬜ open | — (22 surfaces it) |
| 20 | [Sync orchestration](20-sync-orchestration.md) | ⬜ open | — |
| 21 | [Missing-file recovery on open](21-missing-file-recovery.md) | ⬜ open | 20 (for request_sync) |
| 22 | [Sync observability](22-sync-observability.md) | ⬜ open | — |
| 23 | [Tombstone GC](23-tombstone-gc.md) | 💤 deferred | 11 (timestamps); design with 24, 25 |
| 24 | [Append-log compaction](24-append-log-compaction.md) | 💤 deferred | 12 (shared rules); design with 23, 25 |
| 25 | [Blob & thumbnail lifecycle](25-blob-thumbnail-lifecycle.md) | 💤 deferred (½1 near-term after 07) | 07; design ½2 with 23, 24 |
| 26 | [Dead-code audit](26-dead-code-audit.md) | ⬜ open | resolve vs 06 first |
| 27 | [Timestamp unit convention](27-timestamp-units.md) | ⬜ open | — |
| 28 | [Server URL consistency](28-server-url-consistency.md) | ⬜ open | — |
| 33 | [Separate self-hosted blob service](33-separate-blob-service.md) | 💤 deferred | 18, 25 |

## HtmlRenderer decoupling program

Separate track from the Backend/sync plans above — different files, no shared dependencies.
Strictly sequential: each plan sets up the signatures the next one uses.

| # | Plan | Status | Depends on |
|---|------|--------|------------|
| 29 | [Flat glyph-metrics table](29-glyph-metrics-table.md) | ⬜ open | clean working tree |
| 30 | [Painter draws by `GlyphId`](30-painter-glyph-id.md) | ⬜ open | 29 |
| 31 | [Host + event/command boundary](31-renderer-host-boundary.md) | ⬜ open | 30 |
| 32 | [Split HtmlRenderer into crates](32-htmlrenderer-crate-split.md) | ⬜ open | 29–31 |

**Prerequisite for the whole track:** commit the in-flight HtmlRenderer refactor first
(44 modified files under `HtmlRenderer/` on top of a `Checkpoint working tree` commit). Plans
29–32 touch many of the same files; a mass `git mv` on a dirty tree produces unreadable diffs.

Plans 29–31 are safe incremental refactors inside the current crate and are worth doing on
their own merits. 32 only locks in work already done — do not reorder it earlier.

## Archived completed plans

| # | Plan | Status |
|---|------|--------|
| 01 | [Singleton apply guard](archive/01-singleton-apply-guard.md) | ✅ done |
| 02 | [Pull-batch closure (deferred FKs)](archive/02-pull-batch-closure.md) | ✅ done |
| 03 | [Dir materialization + markers](archive/03-dir-materialization.md) | ✅ done |
| 04 | [`dir_state` singleton consolidation](archive/04-dir-state-singleton.md) | ✅ done |
| 05 | [Dir delete cascade](archive/05-dir-delete-cascade.md) | ✅ done |

## Suggested order

1. **14, 15** — known bugs on the critical path (small fixes).
2. **12** — convergence harness, so everything after is property-checked.
3. **6, 7, 9** — main feature/robustness work (10 and 11 ride along with whatever
   touches the same files).
4. **13** — disk-tier harness (wants 9, 14, 15 done).
5. **19, 20, 22** — user-facing sync reliability (auth expiry, orchestration, status).
6. **16, 17, 18** — transport improvements.
7. **26, 27, 28** — hygiene, anytime.
8. **23, 24, 25** — dormant by design; each file states its wake-up condition.
9. **33** — optional deployment split; wake only when storage capacity, bandwidth,
   or independent scaling justifies a second service.

## Conventions

- Plans state decisions, not options — implementing sessions should not re-open settled
  questions (e.g. `.jpg` extension, DB resolution over hex-derivation, remove-wins ties).
- Singleton payloads must always be full entity state (build-from-scratch rule, plan 04).
- "Which book?" → opaque `BookId`; "which bytes?" → opaque `ContentHash`. `BookId` owns
  parsing and its canonical decimal-string encoding for SQLite, sync, JSON, and sessions;
  callers only carry the type. `ContentHash` accepts only full lowercase BLAKE3 digests;
  missing legacy values use `Option<ContentHash>` and synthetic tombstone keys stay strings.
  Never mix the identities or their encodings (plan 07).
- Blob persistence uses closed `BlobKind`, `TransferJobKind`, and `BlobState` domains. Raw
  storage strings and integers are confined to the SQL adapter; callers request named state
  transitions rather than supplying magic values.
- Pre-release assumption: server event log and dev databases may be wiped instead of writing
  legacy-format migration code (plans 04, 06).
- Each plan carries its own test list; convergence-relevant changes should also run plan 12's
  harness once it exists.
- HtmlRenderer layering (plans 29–32): `kurbo`/`peniko` are the geometry/color boundary — do
  not hand-roll replacements, and do not reach for them via `floem::` re-exports. Layout reads
  glyph metrics from a flat table, never through a trait object. The production crate graph is
  acyclic; dev-dependency cycles are deliberate (plan 32, decision 2).
