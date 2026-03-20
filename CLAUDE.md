# CLAUDE.md — brioche-merge-manager

This file gives Claude Code the context needed to work effectively in this codebase.

---

## What this project is

A Rust TUI application for managing GitHub merge queues on
`brioche-dev/brioche-packages`. It shows open PRs that are ready to merge or
failed merging, and lets the user queue/retry them via GitHub's native merge
queue — all from the terminal.

---

## Build & run

```bash
cargo build                        # debug build
cargo build --release              # release build
cargo run                          # run (requires GITHUB_TOKEN in env or .env)
```

There are no tests yet. `cargo build` is the verification step.

## End-of-session checklist

Run both of these before finishing any session:

```bash
cargo clippy -- -D warnings        # must be clean
cargo fmt --check                  # must produce no diff
```

Fix any issues before stopping. If `cargo fmt --check` fails, run `cargo fmt` to fix it.

---

## Project structure

```
src/
├── main.rs              # Entry point: loads config, inits tracing, calls tui::run
├── config.rs            # Config::from_env() — reads GITHUB_TOKEN, GITHUB_OWNER, GITHUB_REPO
├── app.rs               # App struct, Filter enum, Action enum, App::update() state machine
├── event.rs             # Event enum (Key/Tick), crossterm polling task
├── tui.rs               # TerminalGuard (raw mode + alternate screen), tokio::select! run loop
├── github/
│   ├── mod.rs           # Re-exports GitHubClient
│   ├── client.rs        # GitHubClient: fetch_managed_prs, enqueue_pr, retry_pr
│   ├── graphql.rs       # All GraphQL — two-phase parallel fetch, enqueue/dequeue mutations
│   ├── rest.rs          # build_pull_requests(): sorts PRs by status then descending number
│   └── models.rs        # PullRequest, PrStatus, MergeableState, CheckRollupState, etc.
└── ui/
    ├── mod.rs           # render(): layout (header/list/detail/legend), header bar, legend panel
    ├── pr_list.rs       # PR list panel: filter tabs, loading gauge, list items
    └── pr_detail.rs     # PR detail panel: fields with │ separator, action hints
```

---

## Key architectural decisions

### Two channels, not one
`tui::run` uses two `tokio::sync::mpsc::unbounded_channel`s:
- `event_tx/rx` — crossterm keyboard events from a `spawn_blocking` poll loop
- `action_tx/rx` — app actions; API tasks send results back through this channel

### Filtering is UI-only
`App::prs` holds **all** PRs unfiltered. `App::visible_prs()` applies
`App::active_filter` at render time. All navigation and actions go through
`visible_prs()` / `selected_pr()` — never index into `app.prs` directly.

### Two-phase parallel GraphQL fetch
`graphql::fetch_all_prs_bulk` works in two phases:
1. Sequential lightweight cursor collection (only `pageInfo` + `totalCount`, no PR fields)
2. All full-data page requests fired in parallel via `tokio::task::JoinSet`

One `reqwest::Client` is shared across all requests in a fetch cycle for connection pooling.
Page size is **50** (not 100) — GitHub returns 502s at 100 for this repo.

### TUI owns the terminal
stdout/stderr are unavailable at runtime. Logging goes to a file via the
`tracing` crate. Never add `println!` or `eprintln!` calls inside the run loop.

---

## Logging

Uses `tracing` + `tracing-subscriber`. No logs are emitted unless `DEBUG_LOG`
is set — the TUI owns the terminal so stdout/stderr cannot be used.

```bash
DEBUG_LOG=/tmp/bmm.log cargo run
RUST_LOG=trace DEBUG_LOG=/tmp/bmm.log cargo run   # includes response bodies
```

- `tracing::debug!` — page-level events, enqueue/retry calls, startup
- `tracing::trace!` — per-PR parse details, full GraphQL response bodies

See the [README tracing section](README.md#tracing-logs) for full details.

---

## Environment variables

| Variable | Required | Default | Notes |
|---|---|---|---|
| `GITHUB_TOKEN` | Yes | — | Needs `repo` scope |
| `GITHUB_OWNER` | No | `brioche-dev` | |
| `GITHUB_REPO` | No | `brioche-packages` | |
| `BROWSER` | No | system default | Used by `webbrowser` crate; supports `%s` placeholder |
| `DEBUG_LOG` | No | — | File path for tracing output |
| `RUST_LOG` | No | `debug` | Filter when `DEBUG_LOG` is set |

---

## UI colors

All secondary/label text uses `Modifier::DIM` on the terminal's default
foreground — **not** a hardcoded `Color::Gray` or `Color::DarkGray`. This
ensures readability on both dark and light terminals. Explicit colors are
reserved for semantic meaning only:

| Color | Meaning |
|---|---|
| `Color::Green` | Ready to merge / success checks |
| `Color::Red` | Failed merge / error checks |
| `Color::Yellow` | In queue / pending checks |
| `Color::Cyan` | Interactive keys / author names |
| `Color::DarkGray` | Decorative borders and separators only |

---

## PR classification logic

```
ReadyToMerge:  mergeStateStatus == CLEAN,  no merge queue entry,  not a draft
FailedMerge:   mergeQueueEntry.state == UNMERGEABLE
               OR mergeStateStatus == BLOCKED with no queue entry
InQueue:       any other merge queue state, or is a draft
```

Display order: `FailedMerge` first, then `ReadyToMerge`, then `InQueue`,
descending by PR number within each group. Enforced in `rest::build_pull_requests`.

---

## Things to avoid

- **Don't use `println!` / `eprintln!`** inside anything called from the run loop — use `tracing::debug!` with `DEBUG_LOG` set.
- **Don't index `app.prs` directly** in UI or action handlers — always go through `app.visible_prs()`.
- **Don't increase GraphQL `first:` beyond 50** — GitHub returns 502s on this repo at higher values.
- **Don't create a new `reqwest::Client` per GraphQL request** in the bulk fetch path — the shared client in `fetch_all_prs_bulk` provides connection pooling.
- **Don't add `octocrab` GraphQL calls** — octocrab's `.graphql()` swallows errors before they can be inspected; all GraphQL goes through the custom `graphql_post` function in `graphql.rs`.
