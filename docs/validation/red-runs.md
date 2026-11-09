# Initial failing assertions

The initial core tests run against the public `prepare` boundary with raw passthrough behavior.

- `maps_original_longest_spans_once_and_preserves_exact_output` returns `POD gee see and pod, tripod` instead of `podGC and POD, tripod`.
- `repository_override_and_conflicts_stay_reviewable` returns `foo` instead of `Repo`.
- `basic_preserves_quotes_literals_negation_and_repetition` retains hesitation tokens and the unmapped phrase.
- `repository_edits_are_only_suggestions` has an empty suggestion list.
- `indexes_paths_and_declarations_with_origins_but_excludes_ignored_and_binary` fails `assert!(r.available)` against the empty index implementation.
- `cancellation_rejects_late_results_and_processing_rejects_recording` returns `Not implemented` from `start`.
- `hotkeys_ignore_repeat_and_push_release_finishes` returns `None` instead of `Some("start")`.
- `copies_and_pastes_once_only_if_same_eligible_focus` records `(0, 0)` copy/paste calls instead of `(1, 1)`.
- `empty_results_leave_clipboard_unchanged_and_failed_pastes_are_not_retried` has no Clipboard fallback status.
- `accepted_suggestion_can_be_undone_without_losing_manual_edits` returns `Not implemented` from `accept_suggestion`.

The corresponding assertions pass after implementation.
