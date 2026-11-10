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

Independent review and subsequent regression tests also reproduce these failures before their fixes.

- `mapped_words_inside_quotes_preserve_other_quoted_fillers` produces `say ' BAR'` instead of `say 'um BAR'`.
- `quoted_occurrence_does_not_suppress_unquoted_suggestion` returns zero suggestions instead of one.
- `pre_cancelled_external_cleanup_does_not_start_a_request` reports endpoint failure instead of `Cancelled`.
- `repeated_accepted_replacements_keep_independent_undo` retains one undo entry instead of two.

Sound-alike matching, recognition hint, and Always replace.

- `spelled_out_acronym_letters_match_an_indexed_identifier` returns no suggestions instead of `("pod gee see", "podGC")`.
- `near_miss_of_a_multiword_identifier_is_suggested` returns no suggestions instead of `("max group", "maxGroups")`.
- `overlapping_fuzzy_matches_keep_only_the_longest_span` returns zero suggestions instead of one.
- `recognizer_hint_lists_mapped_text_then_common_identifiers` returns an empty hint instead of `Vocabulary: podGC, maxGroups, readFile.`.
- `recognizer_hint_stays_within_its_budget_and_keeps_whole_terms` fails `hint.ends_with('.')` on the empty hint.
- `recognizer_receives_the_vocabulary_hint_as_its_initial_prompt` finds no `prompt` field in the recognizer request.
- `remembered_mapping_replaces_the_heard_phrase_on_the_next_recording` panics because `mapping_for` returns an error.
- `offers to remember an accepted repository suggestion as a mapping` finds no button named "Accept and always replace".
