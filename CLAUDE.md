# Lede — Project Constitution

## What is Lede
Persistent local desktop workspace for data curiosity.
Tagline: "Don't bury the lede in your data."
Target user: Non-technical people with CSVs who want answers, not infrastructure.
Product values: SLICE — Simple, Local, Instant, Correct, Ever Yours.

## Core Architectural Principle
**Rust owns the data. Frontend owns the pixels.**
- All CSV parsing, classification, aggregation in Rust
- Frontend receives pre-computed numbers only
- Never send raw CSV to frontend
- Never compute data in JavaScript

## Tech Stack
- Tauri — desktop shell
- Rust — all data logic (csv, serde, llama-cpp-rs crates)
- React + TypeScript — UI only
- Observable Plot — chart rendering
- Tiptap — prose editor
- DuckDB-WASM — preloaded on start for ad-hoc queries

## Specs (read when relevant)
- Column classification: .claude/specs/column-types.md
- View ontology + suggestion algorithm: .claude/specs/view-ontology.md
- .lede file format: .claude/specs/file-format.md
- AI layer architecture: .claude/specs/ai-layer.md

## Performance Requirements
- Under 1 second to first render for files under 50MB
- Progressive rendering — summary_card first
- DuckDB-WASM preloaded silently on app open

## Testing Requirements
- Every Rust function has unit tests
- Run cargo test after every Rust change
- Test fixtures in tests/fixtures/: shopify.csv, marketing.csv, survey.csv
- Never commit with failing tests

## Verification Rules
You cannot reliably verify GUI output. For visual steps, list
exactly what the user should check. Never mark UI steps complete
based on compilation alone.

## UI Decisions
Make all decisions independently. Minimal, calm, focused aesthetic.
Muted colors, generous whitespace, monospace for data, readable
sans for prose. Do not ask for approval on visual choices.

## Git Discipline
- cargo test must pass before any commit
- Commit format: feat/fix/test/refactor: [what]
- End every session: git add -A && git commit -m "feat: session N complete — [summary]"
- Never leave a session with uncommitted work
