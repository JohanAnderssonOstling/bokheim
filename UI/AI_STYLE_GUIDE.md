# AI Code Style Guide

## Function Signature Style

Use this layout for non-trivial functions:

```rust
fn function_name(
param_name_1                     : Type1,
param_name_2                     : Type2)
-> ReturnType
{
    // body
}
```

Rules:
1. One parameter per line.
2. No indentation on parameter lines.
3. Align `name : type`.
4. Return type on its own line.
5. Opening brace on its own line.

## Branching Style

Rules:
1. Prefer `match` over `if/else` for branch selection.
2. Use `if` only for guard checks and early returns.
3. Early returns must be one line.
4. Group multiple early returns together at the top.
5. Branches decide; they should not execute heavy operations.

Example:

```rust
fn select_plan(
input                            : Input)
-> Plan
{
    if input.items.is_empty() { return Plan::Empty; }
    if !input.enabled { return Plan::Disabled; }

    match input.mode {
        Mode::Fast => plan_fast(input),
        Mode::Safe => plan_safe(input),
    }
}
```

## Function Taxonomy

1. Orchestrator: reads like a checklist.
2. Branch: answers “what should we do?”
3. Executor: answers “how do we do it?”
4. Pure Helper: reusable transformation with no side effects.
5. Gateway Executor: concrete IO boundary operation (db/network/fs).

Rules:
1. Orchestrator composes steps in order, with minimal logic.
2. Branch focuses on conditional selection and delegates work.
3. Executor performs concrete operations and side effects.
4. Pure Helper must not mutate external state or perform IO.
5. Gateway Executor must perform IO but must not choose workflow policy.

## End-to-End Example

```rust
fn run_sync(
ctx                              : SyncContext)
-> SyncResult
{
    if !ctx.enabled { return SyncResult::Skipped; }

    let plan = choose_sync_plan(ctx);
    match plan {
        SyncPlan::NoOp => SyncResult::NoChanges,
        SyncPlan::Defer => SyncResult::Deferred,
        _ => execute_sync_plan(plan),
    }
}

fn choose_sync_plan(
ctx                              : SyncContext)
-> SyncPlan
{
    if ctx.pending_items == 0 { return SyncPlan::NoOp; }

    match (ctx.network, ctx.battery) {
        (Network::Offline, _) => SyncPlan::Defer,
        (Network::Metered, Battery::Low) => SyncPlan::Defer,
        (Network::Metered, _) => SyncPlan::MetadataOnly,
        (Network::Unmetered, _) => SyncPlan::Incremental,
    }
}

fn execute_sync_plan(
plan                             : SyncPlan)
-> SyncResult
{
    let data = execute_download(plan)?;
    execute_persist(data)
}
```

## Formatting Policy

1. Do not run automated formatters on files that use custom signature layout.
2. Preserve one-parameter-per-line aligned signatures exactly as written.
3. If formatter usage is required elsewhere, scope it to files that do not use this style.
4. If needed, protect sections or files with formatter-skip annotations.

## Closure Locality

Rules:
1. Prefer local closures when they are short, single-use, and tied to one composition site.
2. Extract closures to named functions only when logic is reused, non-trivial, or domain-significant.
3. Keep business decisions out of local inline closures; delegate to Branch or Executor functions.
