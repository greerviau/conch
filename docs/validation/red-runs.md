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

# Update check and install

Tests written before the update logic reproduce these failures.

- `automatic_checks_run_at_launch_and_then_when_the_scheduled_time_arrives` returns false for a check that has never run.
- `a_failed_check_is_retried_within_the_hour_and_a_successful_one_in_a_day` returns the current instant instead of one hour or one day later.
- `installing_waits_until_no_recording_or_result_is_in_progress` returns no reason during a recording.
- `the_menu_item_offers_the_next_action_for_each_status` returns an empty label.
- `only_one_check_or_install_runs_at_a_time` fails to compile because `Tracker` has no `begin` or `set`; review found that checking for a running update and starting one were separate steps.
- `automatic_update_checks_are_on_by_default_and_for_settings_saved_before_they_existed` fails to compile because `Settings` has no `check_updates` field.
- The harness tests for `createUpdaterArtifacts`, the updater public key, `scripts/check-version.mjs`, and `scripts/verify-signing.sh` fail because the configuration keys and scripts do not exist.
- The five interface tests in `src/App.test.tsx` for the update switch, version line, check button, install button, and failure message cannot find their controls.

# License

- The harness test for the MIT license fails because `LICENSE` does not exist and neither manifest has a `license` field.

# One-line installer

- The eight `tests/install.test.mjs` tests fail because `scripts/install.sh` does not exist.

# Recognition models and voice detection

- The Rust tests `recognizer_starts_with_voice_detection_so_silence_yields_no_text`, `recognition_without_the_voice_detection_model_asks_for_the_download`, `recognition_models_are_chosen_by_id_and_pinned`, `downloading_a_recognizer_also_fetches_the_missing_voice_detection_model`, and `the_recognizer_defaults_to_tiny_en_and_survives_settings_saved_before_the_choice` fail to compile because `VAD`, `RECOGNIZERS`, `recognizer`, `missing`, `Model::id`, and `Settings::recognizer` do not exist.
- The Rust tests `settings_must_name_an_offered_recognition_model` and `recording_needs_the_selected_recognizer_and_the_voice_detection_model` were written in the same step as `choices_valid` and `recognition_model`, so no separate failing run was recorded for them.
- Review found two more gaps, and each got a failing test first. `an_unknown_stored_recognizer_loads_as_the_default` failed because `load` returned `"huge"`. The interface test for a copy with the recognizer but no voice detection model failed because the setup card said "Download the 78 MB speech model."
- Five interface tests in `src/App.test.tsx` fail: the setup download sends `{ model: 0 }` instead of `{ id: "tiny.en" }`, recording stays enabled without the voice detection model, the setup text names 78 MB for every model, and the Settings page has no Recognition model menu.
