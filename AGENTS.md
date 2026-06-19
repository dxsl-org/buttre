## Project
buttre — Cross-platform Vietnamese input method engine (Rust, TSF/IBus/macOS IMKit).

## Tooling
- Build: `cargo build --release`
- Test:  `cargo test` / `cargo test --package buttre-engine`
- Lint:  `cargo clippy --all-targets --all-features`
- Fmt:   `cargo fmt`

## Safety Rules
- NEVER commit secrets (.env, API keys, credentials)
- NEVER force-push to main/master without explicit user confirmation
- NEVER use `unwrap()` / `expect()` / `panic!()` in library code — use `Result`/`Option`/`?`
- NEVER ignore failing tests to make CI green
- NEVER use `unsafe` in `buttre-engine` or `buttre-core`; document every `unsafe` block in `buttre-platform` with `// SAFETY:` comments

## Vietnamese Input Rules (engine changes)
When changing how Vietnamese is typed (Telex/VNI/VIQR rules, tone placement, transforms, fallback, validation):
- **Flow through the 7-stage pipeline** — implement the change in the stage that owns it (`crates/buttre-engine/src/pipeline/` + `compose/`). Do NOT add side channels or platform-layer special cases. See [docs/PIPELINE_ARCHITECTURE.md](docs/PIPELINE_ARCHITECTURE.md).
- **Write a GENERAL algorithm**, config-driven from the rule/tone tables — never hardcode a single word, syllable, or keystroke string. If you catch yourself matching a literal like `"won"` or `"nghieng"`, generalize it (e.g. via phonology validity, vowel-group counting, coda tables).
- **Hardcoding is a last resort** — only when a general solution is genuinely impossible, and then it MUST carry a `// HARDCODE:` comment explaining why no general rule works and what would remove it.
- **Prefer the phonology tables** in `crates/buttre-engine/src/pipeline/validation.rs` (onsets/nuclei/codas/combinations) as the source of truth for "is this valid Vietnamese". Extend the tables rather than special-casing in logic.
- **Cover every method** — a rule change must be validated against Telex, VNI, and VIQR (and Nôm where relevant), with golden snapshots regenerated and reviewed (`cargo run -p buttre-core --example gen_golden`; diff must contain only intended changes).

## Release Checklist
When bumping the version (in `Cargo.toml` files or during a release):
1. Update the version string in the **"Hướng dẫn"** (help dialog) screen:
   - File: `crates/buttre-platform/src/shared/ui/help_dialog.rs`
   - Line: the `"Phiên bản: X.Y.Z"` entry inside the `message` string literal
2. Update `CHANGELOG.md` with the new version section
3. Confirm all workspace crates share the same version

## Docs
- [docs/00-context.md](docs/00-context.md) — full project context, AI agent quick-start
- [docs/01-architecture.md](docs/01-architecture.md) — system architecture
- [docs/02-coding-guide.md](docs/02-coding-guide.md) — code standards and patterns
- [docs/ROADMAP.md](docs/ROADMAP.md) — current phase and priorities
- [docs/PIPELINE_ARCHITECTURE.md](docs/PIPELINE_ARCHITECTURE.md) — 7-stage input pipeline
- [docs/FFI_SAFETY_GUIDE.md](docs/FFI_SAFETY_GUIDE.md) — unsafe FFI rules
