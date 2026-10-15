# Boiler Room

Boiler Room is a local-first desktop app for building and orchestrating AI agent factories. Design workflows, assign specialized workers powered by coding agents like Claude Code, monitor execution in real time, and send work through automated QA and validation loops. Turn ideas into finished software, with full visibility and control.

> **Status: early development (v0.1).** The core foundation is in place; the engine, agent adapters and desktop UI are being built. See the [roadmap](#roadmap).

## What it is

Boiler Room is a control room that wraps the coding-agent CLIs you already use. It is not an AI itself and never calls model APIs directly:

- It launches installed agent CLIs (Claude Code first) as supervised processes.
- Authentication stays inside each CLI. No API key is needed, and the app never reads tokens, cookies or credentials.
- You describe a feature in a persistent chat harness. A configurable graph of specialized workers (intake, planner, implementers, QA, validator, reviewer) does the work while you watch, inspect and intervene.

## Principles

- **Local-first:** factories, runs, logs and artifacts are stored on disk by default.
- **Provider-agnostic:** the engine talks to agents only through an adapter interface with declared capabilities.
- **Workflow-driven:** factories are configurable graphs, not hardcoded pipelines.
- **Engine-enforced:** the engine, not the model, controls transitions, retry budgets, dependencies and completion. Rework loops are always bounded.
- **Observable:** every meaningful transition is a structured event, tagged as coming from the provider or from the app.
- **Recoverable:** runs resume after a restart without repeating completed work.
- **Safe:** workers get explicit tool permissions and bounded execution.
- **Testable:** the core runs headless against a fake adapter, with no network and no touching of your projects.

## Tech stack

| Layer | Choice |
|---|---|
| Desktop shell | Tauri 2 (Linux, macOS, Windows) |
| Frontend | React, TypeScript, Vite, Tailwind, React Flow |
| Core | Rust (`crates/boiler-core`, no UI dependencies) |
| Persistence | SQLite via rusqlite, versioned migrations |
| Tests | `cargo test`, Vitest + React Testing Library |

## Repository layout

```
crates/boiler-core/   engine, domain model, graph validation, events, SQLite store
boiler-room-vault/    Obsidian vault with project diagrams (purpose, stack, roadmap, progress)
Makefile              common tasks (run `make help`)
```

The Tauri shell (`src-tauri/`) and React UI (`src/`) arrive in milestone 0.4.

## Getting started

Requirements: Rust (stable, via [rustup](https://rustup.rs)), `make`. For the frontend later: Node 22 and pnpm.

```bash
make check    # format check, clippy (warnings are errors), all tests
make help     # list every task
```

The desktop app additionally needs the Tauri system libraries. On Debian/Ubuntu:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libssl-dev build-essential pkg-config
```

## Roadmap

| | Milestone | Scope |
|---|---|---|
| ✅ | 0.1 Foundation | workspace, domain model, graph validation, events, SQLite store |
| 🔧 | 0.2 Engine | scheduler, retry and rework budgets, pause/resume/cancel, recovery, fake adapter |
| | 0.3 Real worker | process runtime, Claude Code adapter, diagnostics, git worktrees |
| | 0.4 Desktop shell | Tauri commands and events, dashboard, persistent harness |
| | 0.5 Observability | run monitor, worker inspector, approvals |
| | 0.6 Designer | React Flow canvas, factory versioning |
| | 0.7 Settings and safety | provider paths, defaults, permission policies |
| | 0.8 Hardening | second provider, cross-platform testing |
| | 0.9 Beta | run history, import/export, installers |
| | 1.0.0 | stable formats, signed builds, auto-update |

The visual roadmap lives in `boiler-room-vault/` (open it as an Obsidian vault with the Excalidraw plugin; start at `Home`).

## License

See [LICENSE](LICENSE).
