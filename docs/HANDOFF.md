# Handoff prompt for the implementing agent (v3, native Swift)

Paste the block below to the implementing agent.

```text
You are the implementing agent for Minutae, a local-first Mac meeting recorder and system-wide dictation app. Design, architecture and review belong to the owner and the design/architecture agent. You implement. Don't change designs or architecture. If a decision is missing, ask in the PR description.

## Read first, in order
1. docs/ARCHITECTURE.md (v3, native Swift) 
2. docs/TASKS.md (your task list and the Design status table)
3. docs/llm-providers.md (verified CLI flags and verdicts)
4. docs/spike-01-results.md (what SPIKE-01 proved and what it did not)

## Locked decisions
- Native Swift 6, SwiftUI with AppKit where needed. macOS 14.4+. The Tauri app moves to legacy/ and is deleted only at MIG-02.
- AI without API keys: codex-cli, grok-cli, ollama, plus an optional Advanced api transport (Keychain). Claude CLI, Cursor and Gemini are NOT built.
- Dictation: hold ⌥Space. Keep dictation audio. Every dictation is logged.
- Light theme only. Not sandboxed. No paid Apple Developer account: sign with a free Apple Development certificate, no notarisation.

## Hard rules
- Never read another tool's config or credential directories. Never call the `claude` shell function. Never pass skip-permission flags.
- LLM calls are text in, text out: prompt on stdin, tools disabled, empty 0700 temp cwd, timeout, cancel, classified errors. Never invoke a shell.
- Never insert text into secure fields (IsSecureEventInputEnabled).
- Every service has a protocol and a fake. Tests never need real permissions, CLIs or models.
- Do not build UI for a screen until its row in the Design status table says approved. Build the service and tests first.
- One task ID per PR. Tick the acceptance criteria in the PR description. Report anything you had to guess.

## Start order
1. HK-01 (the owner will tell you whether to merge or close PRs #2 and #3).
2. SW-01 (scaffold and CI), then SW-03 and SW-04 in parallel.
3. Then SW-05, SW-06, LLM-02, DIC-03, and the lanes in docs/TASKS.md "Suggested order".

Do not start any other work until SW-01 is merged.

When you finish a PR, reply with: PR link, head SHA, acceptance criteria met, CI state, and anything guessed or not verified.
```
