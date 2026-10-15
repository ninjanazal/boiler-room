# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Boiler Room is a local-first desktop app (Tauri 2 + React + Rust) that orchestrates configurable graphs ("factories") of AI coding-agent workers. It **wraps agent CLIs** (Claude Code first) as supervised child processes. It never calls model APIs directly, needs no API key, and must never read tokens, cookies or credentials; authentication stays inside each CLI.

Current state: milestone 0.1 (Foundation) is done, 0.2 (Engine) is next. Only `crates/boiler-core` exists. `src-tauri/` and the React `src/` are planned for milestone 0.4. The roadmap is in `README.md`.

## Commands

Rust and Node are installed per-user, and the Makefile puts `~/.cargo/bin` on `PATH`. Run `make help` for the full list.

```bash
make check                      # CI gate: fmt --check, clippy -D warnings, all tests
make test                       # cargo test --workspace (+ pnpm test once a frontend exists)
cargo test -p boiler-core store::tests::approvals_resolve_once   # single test
cargo test -p boiler-core graph # all tests in a module
make dev / make app             # Tauri dev / bundles; no-ops until src-tauri/ exists
```

Makefile targets with a `## description` comment appear in `make help`; `##@ Name` starts a section.

## Architecture

`crates/boiler-core` must stay **free of Tauri/UI dependencies** so the engine is fully testable headless with `cargo test`. The future `src-tauri` crate is only a thin command/event layer over it.

How the pieces fit (spread across files):

- **`model.rs`** holds the domain types. `FactoryDef` is a graph of `Node`s (kind `Worker` or `Gate`) and `Edge`s (kind `Dependency` or `Rework`). Every worker returns a structured `WorkerResult`.
- **`graph.rs`**: `Graph::new` is the single validation gate and returns a `GraphError` for each rule violation. Dependency edges must be acyclic. Rework edges must come from a gate, point to an ancestor of that gate, and have `max_traversals > 0`. Every worker needs `timeout_secs > 0`. Loops are bounded by construction, not by trusting the model.
- **`events.rs`**: all execution is reported as append-only structured events. Each event is tagged `Source::Provider` or `Source::Application`, so the UI can never present an app-generated message as real provider activity. Do not fabricate tool calls or reasoning.
- **`store.rs`**: SQLite via rusqlite with `rusqlite_migration` (schema in `migrations/*.sql`, `PRAGMA user_version` tracks the version). Factory versions are immutable, and runs reference the version they started with. Events have a global `seq`, so consumers resume with `events_for_run(run, after_seq)`. Provider session IDs are stored so a worker can be resumed after a restart without repeating side effects. Multi-row writes that must be atomic use a transaction (see `create_run` and the private `insert_event`).

Design rules the remaining milestones must follow (from the product spec):

- The **engine**, not the LLM, enforces transitions, retry/rework budgets, dependencies and completion. On restart it rebuilds from the event log and never re-runs completed or side-effecting tasks; interrupted tasks become `needs_review`.
- Providers go behind an adapter trait with declared **capabilities**. Unsupported settings must be rejected, not silently ignored. A **fake adapter** (no network, never touches user projects) is required for tests and dev.
- Claude Code adapter: spawn the installed `claude` in headless mode (`-p --output-format stream-json --verbose`; also `--session-id/--resume`, `--model`, `--effort`, `--permission-mode`, `--allowedTools/--disallowedTools`, `--json-schema`). Verify flags against `claude --help` before relying on them. Missing binary yields a setup diagnostic.
- Parallel coding workers get isolated git worktrees.
- Never store secrets in plaintext config files.

## Obsidian vault (`boiler-room-vault/`)

Project docs are **Excalidraw-only** notes (diagrams, not prose): `Home`, `Purpose`, `Tech Stack`, `Architecture`, `Roadmap 1.0`, `Progress`. Keep them updated as milestones complete (`Progress` and `Roadmap 1.0` in particular).

- The notes are generated: no generator script is checked in, so recreate the Excalidraw JSON programmatically (the plain-JSON `.excalidraw.md` format works) rather than hand-writing it.
- Obsidian re-saves open notes in compressed form (`compressed-json`) and can overwrite external writes made while a note is open. Before regenerating, decompress and check whether `version > 1` on elements, which means the user edited them. Never silently overwrite their edits.
- `.obsidian/workspace*.json` is gitignored.
