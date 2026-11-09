# Conch implementation handoff

The user authorized creating this project and launching a new T3 thread to execute `docs/plans/2026-11-09-conch.md`. Proceed with implementation. The spec's draft wording does not require another approval before starting.

## Settled decisions

- Name: Conch. The user knowingly accepted overlap with existing products. Do not restart naming research.
- Free core operation, offline after model downloads. Optional user-configured remote cleanup is allowed, with no required API key or subscription.
- Desktop support across macOS, Windows, and Linux. Use Tauri 2, TypeScript/React, and Rust. The earlier Swift/macOS-only proposal is superseded.
- Configured repositories supply context. No editor or agent integrations in v1.
- Configurable global hotkey and push-to-talk or toggle recording.
- Automatically paste into an eligible focused text field, otherwise retain the output in the clipboard. Never auto-submit. Use clipboard fallback when focused-field detection is uncertain.
- Explicit global/repository mappings, repository filenames and symbols, correction conflict handling, and raw transcript recovery.
- Basic local filler removal and optional local S1-mini enhanced cleanup. Conch manages models and runtime. Validate preservation of technical identifiers, negation, and constraints.

## Execution

Read the spec and glossary first. Follow the implement, tdd, and doc-audit skills where available, and read `~/CODING-STANDARDS.md` and any repository standards before code changes. Complete experiments first where they resolve design uncertainty, then implement the ordered work items. Keep the spec and glossary synchronized with decisions.

This is a newly initialized local Git project with no remote configured. Do not create a GitHub repository, publish, or upload source merely to satisfy a PR workflow. Make local implementation reviewable and report remote/PR availability accurately. Do not send messages to other people or comment on PRs without user instruction.

Use T3-managed workspace strategy if another checkout is needed. Do not create a second top-level thread unless requested. The launching thread binds this execution to its own worktree.

The current environment is macOS. Run checks available here and add cross-platform CI/build configuration where needed. Distinguish tested behavior from untested platform support. Do not report Windows/Linux success from configuration alone. No representative user recordings or target hardware performance thresholds have been supplied. Use reproducible fixtures and synthetic inputs for initial experiments, mark their limits, and request representative recordings only when needed for remaining voice-quality validation. Do not fabricate measurements or treat missing recordings as a reason to stop all implementation.

## User conventions

- Use uv for Python tooling, never plain pip.
- Ignore backward compatibility unless requested.
- Do not add agents as commit co-authors or generated-by text to PR descriptions.
- Fix lint errors, test failures, and flaky tests encountered during the work, including unrelated failures, as instructed by the user.
- Be direct and concise. Use ASCII prose. Verify claims and report uncertainty. Do not invent test results, platform behavior, benchmarks, or model capabilities.
