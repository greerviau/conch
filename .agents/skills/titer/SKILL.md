---
name: titer
description: Use after changing code in this repo and before committing or opening a PR, to check with `titer check --json` whether the change's tests test the change. Also use when titer reports survivors, unproven changes, or weakened tests.
---

<!-- titer-version: 0.1.3 -->

# titer

This repo uses titer to check whether the tests in a change test the change. titer gathers evidence; it cannot show that a change is correct. Run it after a code change, before you commit or open a PR.

## Run

Run the version the repo pins:

- `npx titer check --json` from the repo root

titer compares the working tree, including untracked files, with the merge-base of `--base` (default: the remote default branch). It never modifies the checkout. A typical run takes under 2 minutes; `--budget 60s` makes it shorter.

## Exit codes

- 0: the gate passes.
- 1: the gate fails. By default this means the change is unproven: no changed test fails on base, and the changed tests kill no mutant of the changed lines.
- 2: the check could not run. Read `gate.reasons`. Common causes are a changed test that fails on head (fix it first), a config error, or a canary that survived (the test run loads code from outside the worktree, for example through an editable install or a Cargo path dependency outside the repo).

## Read the report

`tests` has one entry per changed test. Its `verdict` says what the test did on the base code:

- `assertion` or `exception`: the test fails on base, so it detects a difference between base and head. The gate counts this as proof. Read `message` and check that the failure is the behavior the change targets; a test can also fail on base for an unrelated reason.
- `missing_symbol`: the test fails on base only because it imports or calls something new. It shows nothing about behavior. Add a test that exercises the behavior through code that exists in base, or rely on mutation results.
- `passes_on_base`: a regression guard. It is fine when mutants show that it pins the changed lines.
- `skipped` or `no_result`: the test did not run. It shows nothing.

`mutants` has one entry per mutant of a changed line. Its `outcome`:

- `killed` or `timed_out`: the changed tests detect this edit.
- `survived`: the changed tests run the line but miss the change. Triage it (see below).
- `not_executed`: no green changed test runs the line. If the line matters, add a test that runs it.
- `caught_by_other`: an older test in the changed test files catches it, but the change's own tests do not.
- `no_result` or `not_run`: nothing ran, or the budget ran out. Not a verdict.

`findings` lists weakened tests: `test_deleted`, `assertion_removed`, `skip_added`, `matcher_loosened`, and `snapshot_changed`. Each one needs a reason in the PR, or a revert.

`crap` scores each changed function by complexity and coverage. It is informational.

## Triage survivors

A survivor is either a real gap or noise. Decide which before you act. When `unchanged_from_base` is true, the mutated code also appears in the base lines that the hunk replaces, so the change may only have moved or reformatted it. Check the base code to confirm, and say so in the PR.

- Real gap: the mutant changes behavior that a user or caller can observe, and no test pins it. Write a test that fails on the mutant and passes on the real code.
- Noise: the mutant is equivalent or does not matter, such as a tuning constant, `return []` versus `return None` where callers treat both as empty, or a cosmetic attribute. Leave it and say why in the PR.

Do not write tests that pin noise just to kill every mutant.

## Rules

- Never weaken a test to make the gate pass. Do not delete or skip tests, remove assertions, loosen matchers, or update snapshots without a reason you state in the PR.
- Do not change `titer.toml` to make a failing check pass.
- If titer reports exit 2, fix the cause instead of skipping the check.
