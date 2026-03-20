# brioche-merge-manager

A terminal UI for managing GitHub merge queues on the
[brioche-dev/brioche-packages](https://github.com/brioche-dev/brioche-packages) repository.

Shows all open PRs, highlights which are **ready to queue** or **removed from the merge queue**, lets you
queue or retry them in GitHub's native merge queue, and opens any PR in your
browser — all without leaving the terminal.

---

## Screenshot

```
 🍞 Brioche Merge Manager   brioche-dev/brioche-packages  ·  312 PRs
╭ Pull Requests ───────────────────────────────────────────────────────╮
│ ▶ Active (312)   Ready (9)   Removed (3)   Queued (47)               │
│                                                                       │
│ ▶ ● #3712  ready  ✓  feat: add python package          @alice        │
│   ● #3698  ready  ✓  chore: bump openssl                @bob         │
│   ● #3685  removed ✗  fix: use correct cmake flags     @carol        │
│   ● #3601  draft     wip: refactor build system        @dave  draft  │
╰───────────────────────────────────────────────────────────────────────╯
╭ ● PR #3712 ──────────────────────────╮╭ Diff  d to scroll ──────────╮
│                                       ││                              │
│   Title  │  feat: add python package  ││  3 files  +47  -12          │
│   Author │  @alice                    ││  ──────────────────────      │
│   Status │  ●  Ready to merge         ││                              │
│   Checks │  ✓  success                ││  ~ src/packages/python.bri  │
│   Review │  ✓  approved               ││  @@ -1,6 +1,8 @@            │
│   URL    │  https://github.com/…      ││  -version = "3.11.0"        │
│                                       ││  +version = "3.12.0"        │
│  r  Add to queue    o  Open in browser  ││  +                        │
╰───────────────────────────────────────╯╰──────────────────────────────╯
  ↑↓ / jk  Navigate    Tab / ⇧Tab  Cycle filter    R  Refresh
  r  Add to queue    o  Open in browser    d  Diff    Ctrl+C  Quit
```

---

## Requirements

- Rust 1.75+
- A GitHub personal access token (see [Token Permissions](#token-permissions) below)

---

## Token Permissions

The `GITHUB_TOKEN` must be a **fine-grained personal access token** with the
following repository permissions for the target repo:

| Permission | Access |
|---|---|
| **Merge queues** | Read and write |
| **Pull requests** | Read and write |
| **Metadata** | Read |
| **Contents** | Read and write |
| **Actions** | Read |

Create one at **GitHub → Settings → Developer settings → Personal access tokens → Fine-grained tokens**.

---

## Installation

```bash
git clone https://github.com/brioche-dev/brioche-merge-manager
cd brioche-merge-manager
cargo build --release
```

The compiled binary will be at `target/release/brioche-merge-manager`.

---

## Configuration

Configuration is via environment variables. Copy `.env.example` to `.env` for
local development, or export them in your shell.

| Variable | Required | Default | Description |
|---|---|---|---|
| `GITHUB_TOKEN` | Yes | — | GitHub fine-grained personal access token (see [Token Permissions](#token-permissions)) |
| `GITHUB_OWNER` | No | `brioche-dev` | Repository owner |
| `GITHUB_REPO` | No | `brioche-packages` | Repository name |
| `BROWSER` | No | system default | Browser command to use when pressing `o` |
| `DEBUG_LOG` | No | — | Path to write trace logs to (see [Tracing Logs](#tracing-logs)) |
| `RUST_LOG` | No | `debug` | Log filter when `DEBUG_LOG` is set (see [Tracing Logs](#tracing-logs)) |

### Example `.env`

```env
GITHUB_TOKEN=ghp_xxxxxxxxxxxxxxxxxxxx
GITHUB_OWNER=brioche-dev
GITHUB_REPO=brioche-packages
```

---

## Usage

```bash
cargo run
# or after building:
./target/release/brioche-merge-manager
```

### Keybindings

**PR list navigation**

| Key | Action |
|---|---|
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `Page Up` | Jump up one page |
| `Page Down` | Jump down one page |
| `Home` | Jump to first PR |
| `End` | Jump to last PR |
| `Tab` | Cycle filter forward |
| `Shift+Tab` | Cycle filter backward |

**Actions**

| Key | Action |
|---|---|
| `r` | Add selected PR to merge queue (works for ready and removed PRs) |
| `o` | Open selected PR in browser |
| `Enter` | Toggle diff panel |
| `d` | Focus/unfocus diff panel for scrolling (when open) |
| `R` | Refresh PR list |
| `Ctrl+C` | Quit |

**Diff panel** (press `d` to open and focus)

| Key | Action |
|---|---|
| `↑` / `k` | Scroll diff up |
| `↓` / `j` | Scroll diff down |
| `Page Up` | Scroll diff up one page |
| `Page Down` | Scroll diff down one page |
| `Home` | Jump to top of diff |
| `End` | Jump to bottom of diff |
| `d` | Unfocus diff (restore PR list navigation) |
| `Enter` | Close diff panel |

### Filters

The tab bar at the top of the PR list cycles through four views:

| Filter | Shows |
|---|---|
| **Active** | All open PRs (default) |
| **Ready** | Non-draft PRs with a clean merge state, not yet queued |
| **Removed** | PRs removed from the merge queue and not currently re-queued |
| **Queued** | PRs currently in the merge queue |

---

## How It Works

On launch (and on `R`) the app fetches all open PRs using a two-phase parallel
GraphQL strategy:

1. **Phase 1 — cursor collection** (sequential, lightweight): pages through
   `pullRequests` fetching only `pageInfo` and `totalCount` to collect all
   page-start cursors cheaply.
2. **Phase 2 — parallel fetch**: fires all full-data page requests
   simultaneously using `tokio::task::JoinSet`, then reassembles them in order.

Each PR's status is derived from `mergeStateStatus`, `mergeQueueEntry`, and
`timelineItems` (merge queue removal events):

| Status | Condition |
|---|---|
| **Ready to merge** | `mergeStateStatus = CLEAN` and not currently in the merge queue |
| **In queue** | Has an active `mergeQueueEntry` (any state) |
| **Removed** | Has a `REMOVED_FROM_MERGE_QUEUE_EVENT` in its timeline and is not currently re-queued |

Draft PRs display their real merge state with a `draft` badge and are excluded
from the **Ready** filter since they cannot be queued.

---

## Tracing Logs

Because the TUI owns the terminal at runtime, logs cannot be written to
stdout or stderr. Instead, set `DEBUG_LOG` to a file path and the app will
append structured traces there using the [`tracing`](https://docs.rs/tracing)
ecosystem.

### Enable logging

```bash
DEBUG_LOG=/tmp/bmm.log cargo run
```

Watch the log in a second terminal:

```bash
tail -f /tmp/bmm.log
```

### Control verbosity with `RUST_LOG`

`RUST_LOG` accepts the standard `tracing-subscriber` filter syntax. When
`DEBUG_LOG` is set but `RUST_LOG` is not, the default level is `debug`.

```bash
# debug level (default)
DEBUG_LOG=/tmp/bmm.log cargo run

# trace level — includes per-PR parse details and full GraphQL response bodies
RUST_LOG=trace DEBUG_LOG=/tmp/bmm.log cargo run

# only this crate's logs at debug, suppress noisy dependencies
RUST_LOG=brioche_merge_manager=debug DEBUG_LOG=/tmp/bmm.log cargo run

# this crate at trace, everything else at warn
RUST_LOG=brioche_merge_manager=trace,warn DEBUG_LOG=/tmp/bmm.log cargo run
```

### Log levels used

| Level | What is logged |
|---|---|
| `DEBUG` | Startup, phase transitions, page counts, enqueue/retry calls |
| `TRACE` | Per-PR parse details, full GraphQL response bodies (verbose) |

### Example output

```
2025-03-19T14:22:01.483Z DEBUG brioche_merge_manager: starting owner=brioche-dev repo=brioche-packages
2025-03-19T14:22:01.612Z DEBUG brioche_merge_manager::github::graphql: collect_page_cursors: done pages=6 total_count=287
2025-03-19T14:22:01.613Z DEBUG brioche_merge_manager::github::graphql: fetch_all_prs_bulk: fetching pages in parallel pages=6 total_count=287
2025-03-19T14:22:02.104Z DEBUG brioche_merge_manager::github::graphql: fetch_pr_page: received nodes page=2 count=50
2025-03-19T14:22:02.201Z DEBUG brioche_merge_manager::github::graphql: fetch_pr_page: received nodes page=0 count=50
2025-03-19T14:22:02.309Z DEBUG brioche_merge_manager::github::client: fetch_managed_prs: done count=287
```

---

## Dependencies

| Crate | Purpose |
|---|---|
| `ratatui` | Terminal UI framework |
| `crossterm` | Cross-platform terminal backend |
| `tokio` | Async runtime |
| `reqwest` | HTTP client for GitHub GraphQL API |
| `octocrab` | GitHub API client (authentication) |
| `serde` / `serde_json` | JSON serialisation |
| `tracing` / `tracing-subscriber` | Structured logging |
| `webbrowser` | Cross-platform browser opening (WSL2-aware) |
| `anyhow` | Error handling |
| `dotenvy` | `.env` file loading |
