# CLAUDE.md — mlir_viewer

## Code readability principles (Karpathy-style)

Write code a human can read and verify quickly. These ten rules, inspired by
Andrej Karpathy's advice on keeping code legible in the LLM era, govern all
code in this repo (Rust, C++, TypeScript):

1. **Write for the reader, not the author.** Optimize for the next person
   skimming the file, not for how fast it was to write.
2. **Simple beats clever.** If a one-liner needs a comment to decode, expand
   it into named steps.
3. **No premature abstraction.** Duplicate once; abstract on the third use.
   A trait/interface with one implementation is a smell.
4. **Locality.** Code that changes together lives together. A reader should
   understand a function top-to-bottom without jumping across files.
5. **Small, focused units.** Files own one responsibility; functions fit on
   a screen. Split when a file stops fitting in your head.
6. **Explicit over implicit.** No hidden control flow, no magic globals, no
   action-at-a-distance. Data flows through visible parameters and returns.
7. **Minimize dependencies.** Every new crate/package must pay rent. Prefer
   the standard library and what the repo already uses.
8. **Delete eagerly (YAGNI).** Dead code, unused flags, and speculative
   hooks come out. Git remembers.
9. **Plain, consistent names.** Full words over abbreviations; the same
   concept gets the same name everywhere (`op_idx` is never also `opIndex`
   within a language's own convention).
10. **Comments say why, not what.** State constraints, invariants, and
    non-obvious reasons the code can't express (e.g. "pointer tokens are
    reused after free — never compare across passes"). Never narrate the
    next line.

## Project conventions

- Milestone workflow: brainstorm → spec (`docs/superpowers/specs/`) → plan
  (`docs/superpowers/plans/`) → subagent-driven implementation. Specs get
  `Status: Approved` before a plan is written.
- Rust toolchain is not on PATH:
  `export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"`.
- Verification gate for any change: `cargo test --workspace`, and for UI
  work `cd ui && npm run typecheck && npx vitest run && npx playwright test`.
- Bulk API payloads are MessagePack; control-plane endpoints are JSON. All
  list/graph responses are budgeted — never return unbounded payloads.
- Commit messages: conventional commits, ending with
  `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
