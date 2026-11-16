# ADR-0009: Typed IPC between the UI and Rust with tauri-specta

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

The GUI (React/TypeScript) calls Rust commands and receives events (session
state, logs). With loose strings and types duplicated by hand, the two sides
drift apart without anyone noticing.

## Decision

- The GUI uses Tauri v2. The commands live in `src-tauri` and are thin: they
  delegate to `crates/core`.
- `tauri-specta` generates the TypeScript bindings for commands and events
  from the Rust types. The version is pinned exactly, because v2 is still a
  release candidate.
- The CLI (`crates/cli`) uses the same `core`. All business logic stays out of
  the UI layer.

## Consequences

- Changing a type in Rust breaks the TypeScript compilation. That is the
  desired behavior.
- Depending on a release candidate is a risk. That is why the version is
  pinned, and an update becomes an isolated PR.

## Alternatives considered

- **`invoke` with hand-written types.** They drift over time.
- **ts-rs.** It generates the types, but not the commands and events.
