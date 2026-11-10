use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Mapping {
    pub phrase: String,
    pub replacement: String,
    pub repository: Option<String>,
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Vocabulary {
    pub text: String,
    pub repository: String,
    pub file: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Cleanup {
    pub fillers: Vec<String>,
    pub excluded: Vec<String>,
    /// Enhanced cleanup: `disabled`, `managed`, `local`, or `remote`.
    pub enhanced: String,
    pub endpoint: String,
    pub model: String,
    pub remote_enabled: bool,
}
impl Default for Cleanup {
    fn default() -> Self {
        Self {
            fillers: vec!["um".into(), "uh".into()],
            excluded: vec![],
            enhanced: "disabled".into(),
            endpoint: String::new(),
            model: String::new(),
            remote_enabled: false,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub mappings: Vec<Mapping>,
    pub vocabulary: Vec<Vocabulary>,
    pub cleanup: Cleanup,
    /// Apply repository suggestions that have one candidate without review.
    pub auto_accept: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Suggestion {
    pub start: usize,
    pub end: usize,
    pub original: String,
    pub candidates: Vec<Vocabulary>,
    pub conflict: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppliedEdit {
    pub start: usize,
    pub end: usize,
    pub original: String,
    pub replacement: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResultText {
    pub raw: String,
    pub text: String,
    pub basic: String,
    pub suggestions: Vec<Suggestion>,
    pub status: String,
    pub edits: Vec<AppliedEdit>,
}
fn phrase_regex(phrase: &str) -> Option<regex::Regex> {
    if phrase.trim().is_empty() {
        return None;
    }
    regex::Regex::new(&format!(
        r"(?i){}",
        phrase
            .split_whitespace()
            .map(regex::escape)
            .collect::<Vec<_>>()
            .join(r"\s+")
    ))
    .ok()
}
fn boundary(text: &str, start: usize, end: usize) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    !text[..start].chars().next_back().is_some_and(word)
        && !text[end..].chars().next().is_some_and(word)
}
#[derive(Default)]
pub struct Masked {
    pub text: String,
    pub literals: Vec<String>,
    pub ranges: Vec<(usize, usize)>,
}
impl Masked {
    fn token(&mut self, literal: &str) -> String {
        let token = format!("CONCHPROTECTED{}X", self.literals.len());
        self.literals.push(literal.into());
        token
    }
    pub fn restore(&self, text: &str) -> Option<String> {
        let pattern = regex::Regex::new(r"CONCHPROTECTED\d+X").unwrap();
        let expected: Vec<_> = (0..self.literals.len())
            .map(|i| format!("CONCHPROTECTED{i}X"))
            .collect();
        let actual: Vec<_> = pattern.find_iter(text).map(|m| m.as_str()).collect();
        if actual != expected {
            return None;
        }
        let mut out = text.to_owned();
        for (token, literal) in expected.iter().zip(&self.literals) {
            out = out.replacen(token, literal, 1);
        }
        Some(out)
    }
}
/// Spans of `"..."`, `` `...` `` and `'...'` on one line. A single quote opens
/// only after a non-word character and closes only before one, so the
/// apostrophes in "don't ... it's" do not pair up.
fn quote_spans(text: &str) -> Vec<(usize, usize)> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut spans = vec![];
    let mut at = 0;
    while let Some(offset) = text[at..].find(['"', '\'', '`']) {
        let start = at + offset;
        let mark = text.as_bytes()[start] as char;
        at = start + 1;
        if mark == '\'' && text[..start].chars().next_back().is_some_and(word) {
            continue;
        }
        let line_end = text[at..].find('\n').map_or(text.len(), |n| at + n);
        let close = text[at..line_end]
            .match_indices(mark)
            .find(|(i, _)| mark != '\'' || !text[at + i + 1..].chars().next().is_some_and(word));
        if let Some((i, _)) = close {
            let end = at + i + 1;
            spans.push((start, end));
            at = end;
        }
    }
    spans
}
pub fn mask(raw: &str, snapshot: &Snapshot) -> Masked {
    let mut spans: Vec<(usize, usize, String)> = vec![];
    let mut groups = std::collections::BTreeMap::<String, Vec<&Mapping>>::new();
    for m in snapshot
        .mappings
        .iter()
        .filter(|m| m.enabled && !m.phrase.trim().is_empty())
    {
        groups
            .entry(
                m.phrase
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase(),
            )
            .or_default()
            .push(m);
    }
    for (phrase, choices) in groups {
        let local: Vec<_> = choices.iter().filter(|m| m.repository.is_some()).collect();
        let options: Vec<_> = if local.is_empty() {
            choices.iter().collect()
        } else {
            local
        };
        let replacements: std::collections::BTreeSet<_> =
            options.iter().map(|m| m.replacement.as_str()).collect();
        if let Some(re) = phrase_regex(&phrase) {
            for m in re
                .find_iter(raw)
                .filter(|m| boundary(raw, m.start(), m.end()))
            {
                let replacement = if replacements.len() == 1 {
                    *replacements.first().unwrap()
                } else {
                    m.as_str()
                };
                spans.push((m.start(), m.end(), replacement.into()));
            }
        }
    }
    spans.sort_by_key(|(s, e, _)| (std::cmp::Reverse(e - s), *s));
    let mut chosen: Vec<(usize, usize, String)> = vec![];
    for span in spans {
        if !chosen.iter().any(|s| span.0 < s.1 && s.0 < span.1) {
            chosen.push(span);
        }
    }
    let quotes = quote_spans(raw);
    for &(quote_start, quote_end) in &quotes {
        let crossing = chosen.iter().any(|(start, end, _)| {
            *start < quote_end
                && quote_start < *end
                && !(*start >= quote_start && *end <= quote_end)
        });
        if crossing {
            continue;
        }
        let mut inside: Vec<_> = chosen
            .iter()
            .filter(|(start, end, _)| *start >= quote_start && *end <= quote_end)
            .cloned()
            .collect();
        inside.sort_by_key(|span| std::cmp::Reverse(span.0));
        let mut literal = raw[quote_start..quote_end].to_owned();
        for (start, end, replacement) in inside {
            literal.replace_range(start - quote_start..end - quote_start, &replacement);
        }
        chosen.retain(|(start, end, _)| !(*start >= quote_start && *end <= quote_end));
        chosen.push((quote_start, quote_end, literal));
    }
    let mut protected: Vec<_> = quotes
        .iter()
        .map(|&(start, end)| (start, end, raw[start..end].to_owned()))
        .collect();
    for v in &snapshot.vocabulary {
        if v.text.is_empty() {
            continue;
        }
        for (start, literal) in raw.match_indices(&v.text) {
            if boundary(raw, start, start + literal.len()) {
                protected.push((start, start + literal.len(), literal.into()));
            }
        }
    }
    for term in &snapshot.cleanup.excluded {
        if let Some(re) = phrase_regex(term) {
            protected.extend(
                re.find_iter(raw)
                    .filter(|m| boundary(raw, m.start(), m.end()))
                    .map(|m| (m.start(), m.end(), m.as_str().into())),
            );
        }
    }
    protected.sort_by_key(|(s, e, _)| (std::cmp::Reverse(e - s), *s));
    for span in protected {
        if !chosen.iter().any(|s| span.0 < s.1 && s.0 < span.1) {
            chosen.push(span);
        }
    }
    chosen.sort_by_key(|s| s.0);
    let mut out = Masked::default();
    let mut at = 0;
    for (start, end, literal) in chosen {
        out.text.push_str(&raw[at..start]);
        out.ranges.push((start, end));
        let token = out.token(&literal);
        out.text.push_str(&token);
        at = end;
    }
    out.text.push_str(&raw[at..]);
    out
}
pub fn basic(masked: &str, cleanup: &Cleanup) -> String {
    if cleanup
        .fillers
        .iter()
        .all(|filler| filler.trim().is_empty())
    {
        return masked.into();
    }
    let mut text = masked.to_owned();
    for filler in &cleanup.fillers {
        if let Some(re) = phrase_regex(filler) {
            let spans: Vec<_> = re
                .find_iter(&text)
                .filter(|m| {
                    boundary(&text, m.start(), m.end())
                        && !text[..m.start()].ends_with(['/', '.', '\\', '-'])
                        && !text[m.end()..].starts_with(['/', '.', '\\', '-'])
                })
                .map(|m| (m.start(), m.end()))
                .collect();
            for (start, mut end) in spans.into_iter().rev() {
                if text[end..].starts_with(',') {
                    end += 1;
                }
                text.replace_range(start..end, "");
            }
        }
    }
    let spaces = regex::Regex::new(r"[ \t]{2,}").unwrap();
    let punctuation = regex::Regex::new(r"\s+([,.;:!?])").unwrap();
    text = spaces.replace_all(&text, " ").into_owned();
    text = punctuation.replace_all(&text, "$1").into_owned();
    text.trim_matches(|c: char| c.is_whitespace() || c == ',')
        .to_owned()
}
fn spoken(text: &str) -> String {
    static CAMEL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let camel = CAMEL.get_or_init(|| regex::Regex::new(r"([a-z0-9])([A-Z])").unwrap());
    camel
        .replace_all(text, "$1 $2")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
/// Adds suggestions for words the recognizer spelled differently from an
/// indexed term. Where two spans overlap, the closer match wins, then the
/// longer span, and an exact word-sequence match from `suggestions` wins over
/// both.
fn add_sound_alikes(
    text: &str,
    snapshot: &Snapshot,
    protected: &[(usize, usize)],
    groups: &mut std::collections::BTreeMap<(usize, usize, bool), Vec<Vocabulary>>,
) {
    let found = crate::spoken::find(text, &snapshot.vocabulary, protected);
    let mut best = std::collections::BTreeMap::<(usize, usize), usize>::new();
    for m in &found {
        best.entry((m.start, m.end))
            .and_modify(|d| *d = (*d).min(m.distance))
            .or_insert(m.distance);
    }
    let mut order: Vec<_> = best.into_iter().collect();
    order
        .sort_by_key(|&((start, end), distance)| (distance, std::cmp::Reverse(end - start), start));
    let mut kept: Vec<(usize, usize)> = vec![];
    for ((start, end), distance) in order {
        if distance > 0 && groups.contains_key(&(start, end, false)) {
            continue;
        }
        let overlaps = |&(s, e): &(usize, usize)| start < e && s < end && (s, e) != (start, end);
        if kept.iter().any(overlaps) || groups.keys().any(|&(s, e, _)| overlaps(&(s, e))) {
            continue;
        }
        kept.push((start, end));
        for m in found
            .iter()
            .filter(|m| (m.start, m.end) == (start, end) && m.distance == distance)
        {
            let term = &snapshot.vocabulary[m.term];
            let candidates = groups.entry((start, end, false)).or_default();
            if !candidates.iter().any(|c| {
                c.text == term.text && c.repository == term.repository && c.file == term.file
            }) {
                candidates.push(term.clone());
            }
        }
    }
}
pub fn suggestions(text: &str, snapshot: &Snapshot) -> Vec<Suggestion> {
    let mut groups = std::collections::BTreeMap::<(usize, usize, bool), Vec<Vocabulary>>::new();
    let mut protection = snapshot.clone();
    protection
        .vocabulary
        .extend(
            snapshot
                .mappings
                .iter()
                .filter(|m| m.enabled)
                .map(|m| Vocabulary {
                    text: m.replacement.clone(),
                    ..Default::default()
                }),
        );
    protection.mappings.clear();
    let protected = mask(text, &protection);
    let normalized = spoken(text);
    for v in &snapshot.vocabulary {
        let phrase = spoken(&v.text);
        if !normalized.contains(&phrase) {
            continue;
        }
        if phrase == v.text.to_lowercase() {
            continue;
        }
        if let Some(re) = phrase_regex(&phrase) {
            for m in re.find_iter(text).filter(|m| {
                boundary(text, m.start(), m.end())
                    && !protected
                        .ranges
                        .iter()
                        .any(|(start, end)| m.start() < *end && *start < m.end())
            }) {
                groups
                    .entry((m.start(), m.end(), false))
                    .or_default()
                    .push(v.clone());
            }
        }
    }
    add_sound_alikes(text, snapshot, &protected.ranges, &mut groups);
    for m in snapshot
        .mappings
        .iter()
        .filter(|m| m.enabled && m.repository.is_some())
    {
        let alternatives: Vec<_> = snapshot
            .mappings
            .iter()
            .filter(|a| {
                a.enabled && a.repository.is_some() && a.phrase.eq_ignore_ascii_case(&m.phrase)
            })
            .collect();
        if !alternatives.iter().any(|a| a.replacement != m.replacement) {
            continue;
        }
        if let Some(re) = phrase_regex(&m.phrase) {
            for found in re
                .find_iter(text)
                .filter(|m| boundary(text, m.start(), m.end()))
            {
                let candidates = groups
                    .entry((found.start(), found.end(), true))
                    .or_default();
                if !candidates.iter().any(|v| {
                    v.text == m.replacement && v.repository == *m.repository.as_ref().unwrap()
                }) {
                    candidates.push(Vocabulary {
                        text: m.replacement.clone(),
                        repository: m.repository.clone().unwrap(),
                        file: "explicit mapping".into(),
                    });
                }
            }
        }
    }
    groups
        .into_iter()
        .map(|((start, end, conflict), candidates)| Suggestion {
            start,
            end,
            original: text[start..end].into(),
            candidates,
            conflict,
        })
        .collect()
}
/// Largest hint sent to the recognizer. whisper.cpp keeps at most 224 prompt
/// tokens, and an identifier costs about three.
pub const RECOGNITION_HINT_BYTES: usize = 400;
/// Longest mapping replacement worth spending hint space on.
const HINT_TERM_BYTES: usize = 40;
/// Builds the recognizer's initial prompt from the context snapshot. Enabled
/// mapping replacements come first, then indexed identifiers that more files
/// declare, then the rest in alphabetical order. Plain words and file paths
/// are left out because the recognizer already spells them correctly.
pub fn recognition_hint(snapshot: &Snapshot) -> String {
    const PREFIX: &str = "Vocabulary: ";
    let mut terms: Vec<&str> = snapshot
        .mappings
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.replacement.trim())
        .filter(|t| !t.is_empty() && t.len() <= HINT_TERM_BYTES && !t.contains('\n'))
        .collect();
    let mut declared = std::collections::BTreeMap::<&str, usize>::new();
    for v in &snapshot.vocabulary {
        if crate::spoken::is_identifier(&v.text) {
            *declared.entry(v.text.as_str()).or_default() += 1;
        }
    }
    let mut ranked: Vec<_> = declared.into_iter().collect();
    ranked.sort_by_key(|&(text, count)| (std::cmp::Reverse(count), text));
    terms.extend(ranked.into_iter().map(|(text, _)| text));
    let mut chosen: Vec<&str> = vec![];
    let mut seen = std::collections::HashSet::new();
    // The prefix and the closing period.
    let mut size = PREFIX.len() + 1;
    for term in terms {
        let separator = if chosen.is_empty() { 0 } else { 2 };
        if size + separator + term.len() > RECOGNITION_HINT_BYTES || !seen.insert(term) {
            continue;
        }
        size += separator + term.len();
        chosen.push(term);
    }
    if chosen.is_empty() {
        return String::new();
    }
    format!("{PREFIX}{}.", chosen.join(", "))
}
pub fn prepare(raw: &str, snapshot: &Snapshot) -> ResultText {
    let masked = mask(raw, snapshot);
    let text = masked
        .restore(&basic(&masked.text, &snapshot.cleanup))
        .unwrap_or_else(|| raw.into());
    let mut result = ResultText {
        raw: raw.into(),
        basic: text.clone(),
        suggestions: suggestions(&text, snapshot),
        text,
        edits: vec![],
        status: "Prepared locally".into(),
    };
    auto_accept(&mut result, snapshot);
    result
}
/// Accepts every repository suggestion whose candidates all spell the same
/// replacement when the snapshot opts in. Each acceptance is recorded as an
/// edit, so Undo restores the heard words. A suggestion that conflicts with
/// mappings, names several different replacements, or overlaps another
/// suggestion stays for review.
pub fn auto_accept(result: &mut ResultText, snapshot: &Snapshot) {
    if !snapshot.auto_accept {
        return;
    }
    let eligible = |at: usize, suggestion: &Suggestion, all: &[Suggestion]| {
        !suggestion.conflict
            && suggestion
                .candidates
                .windows(2)
                .all(|pair| pair[0].text == pair[1].text)
            && !suggestion.candidates.is_empty()
            && !all.iter().enumerate().any(|(other, s)| {
                other != at && suggestion.start < s.end && s.start < suggestion.end
            })
    };
    while let Some(index) = result
        .suggestions
        .iter()
        .enumerate()
        .position(|(at, s)| eligible(at, s, &result.suggestions))
    {
        if accept_suggestion(result, index, 0).is_err() {
            result.suggestions.remove(index);
        }
    }
}

pub fn validate_enhanced(before: &str, after: &str, masked: &Masked) -> Option<String> {
    if before.contains("CONCHPROTECTED") && masked.literals.is_empty() {
        return None;
    }
    if after.trim().is_empty() && !before.trim().is_empty() {
        return None;
    }
    let constraints = regex::Regex::new(
        r"(?i)\b(?:not|no|never|without|must|only|before|after|at least|at most|don't|can't|cannot|won't|shouldn't|mustn't|\d+(?:\.\d+)?)\b",
    )
    .unwrap();
    let extract = |s: &str| {
        constraints
            .find_iter(s)
            .map(|m| m.as_str().to_lowercase())
            .collect::<Vec<_>>()
    };
    if extract(before) != extract(after) {
        return None;
    }
    masked.restore(after)
}

pub fn edit_result(result: &mut ResultText, text: String) {
    result.edits.retain_mut(|edit| {
        let positions: Vec<_> = text
            .match_indices(&edit.replacement)
            .map(|(at, _)| at)
            .collect();
        if positions.len() != 1 {
            return false;
        }
        edit.start = positions[0];
        edit.end = edit.start + edit.replacement.len();
        true
    });
    result.suggestions.retain_mut(|suggestion| {
        let positions: Vec<_> = text
            .match_indices(&suggestion.original)
            .map(|(at, _)| at)
            .collect();
        if positions.len() != 1 {
            return false;
        }
        suggestion.start = positions[0];
        suggestion.end = suggestion.start + suggestion.original.len();
        true
    });
    result.text = text;
}
fn replace_known(result: &mut ResultText, start: usize, end: usize, replacement: &str) {
    let shift = replacement.len() as isize - (end - start) as isize;
    result.text.replace_range(start..end, replacement);
    result.edits.retain_mut(|edit| {
        if edit.start < end && start < edit.end {
            return false;
        }
        if edit.start >= end {
            edit.start = (edit.start as isize + shift) as usize;
            edit.end = (edit.end as isize + shift) as usize;
        }
        true
    });
    result.suggestions.retain_mut(|suggestion| {
        if suggestion.start < end && start < suggestion.end {
            return false;
        }
        if suggestion.start >= end {
            suggestion.start = (suggestion.start as isize + shift) as usize;
            suggestion.end = (suggestion.end as isize + shift) as usize;
        }
        true
    });
}
pub fn accept_suggestion(
    result: &mut ResultText,
    index: usize,
    candidate: usize,
) -> Result<(), String> {
    let suggestion = result
        .suggestions
        .get(index)
        .ok_or("Suggestion is unavailable")?
        .clone();
    let replacement = suggestion
        .candidates
        .get(candidate)
        .ok_or("Candidate is unavailable")?
        .text
        .clone();
    if result.text.get(suggestion.start..suggestion.end) != Some(&suggestion.original) {
        return Err("Suggestion is stale".into());
    }
    result.suggestions.remove(index);
    replace_known(result, suggestion.start, suggestion.end, &replacement);
    result.edits.push(AppliedEdit {
        start: suggestion.start,
        end: suggestion.start + replacement.len(),
        original: suggestion.original,
        replacement,
    });
    Ok(())
}
/// The explicit mapping that makes a repository suggestion apply by itself on
/// later recordings: the phrase as heard, replaced by the chosen candidate.
/// The mapping is scoped to the candidate's repository.
pub fn mapping_for(result: &ResultText, index: usize, candidate: usize) -> Result<Mapping, String> {
    let suggestion = result
        .suggestions
        .get(index)
        .ok_or("Suggestion is unavailable")?;
    if suggestion.conflict {
        return Err("Resolve a mapping conflict in Settings".into());
    }
    let candidate = suggestion
        .candidates
        .get(candidate)
        .ok_or("Candidate is unavailable")?;
    Ok(Mapping {
        phrase: suggestion
            .original
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase(),
        replacement: candidate.text.clone(),
        repository: Some(candidate.repository.clone()).filter(|id| !id.is_empty()),
        enabled: true,
    })
}
pub fn undo_edit(result: &mut ResultText, index: usize) -> Result<(), String> {
    let edit = result
        .edits
        .get(index)
        .ok_or("Edit is unavailable")?
        .clone();
    if result.text.get(edit.start..edit.end) != Some(&edit.replacement) {
        return Err("Edited span changed; undo manually".into());
    }
    result.edits.remove(index);
    replace_known(result, edit.start, edit.end, &edit.original);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mapping(phrase: &str, replacement: &str, repository: Option<&str>) -> Mapping {
        Mapping {
            phrase: phrase.into(),
            replacement: replacement.into(),
            repository: repository.map(str::to_owned),
            enabled: true,
        }
    }
    #[test]
    fn repeated_accepted_replacements_keep_independent_undo() {
        let snapshot = Snapshot {
            vocabulary: vec![Vocabulary {
                text: "maxGroups".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut result = prepare("max groups and max groups", &snapshot);
        accept_suggestion(&mut result, 0, 0).unwrap();
        accept_suggestion(&mut result, 0, 0).unwrap();
        assert_eq!(result.edits.len(), 2);
        undo_edit(&mut result, 0).unwrap();
        assert_eq!(result.text, "max groups and maxGroups");
    }
    #[test]
    fn quoted_occurrence_does_not_suppress_unquoted_suggestion() {
        let snapshot = Snapshot {
            vocabulary: vec![Vocabulary {
                text: "maxGroups".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let result = prepare("'max groups' and max groups", &snapshot);
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].start, 17);
    }
    #[test]
    fn basic_leaves_apostrophes_in_contractions_alone() {
        let text = "I don't know what it's doing and we can't tell";
        assert_eq!(prepare(text, &Snapshot::default()).text, text);
    }
    #[test]
    fn contractions_are_not_masked_as_quotes() {
        let text = "I don't know what it's doing";
        let masked = mask(text, &Snapshot::default());
        assert_eq!(masked.text, text);
        assert!(masked.literals.is_empty());
    }
    #[test]
    fn single_quotes_after_contractions_still_mask_as_quotes() {
        let text = "I don't know 'max groups' and it's 'fine'";
        let masked = mask(text, &Snapshot::default());
        assert_eq!(masked.literals, ["'max groups'", "'fine'"]);
        assert_eq!(
            masked.text,
            "I don't know CONCHPROTECTED0X and it's CONCHPROTECTED1X"
        );
    }
    #[test]
    fn apostrophes_inside_single_quotes_stay_in_the_quote() {
        let masked = mask("say 'it's fine' now", &Snapshot::default());
        assert_eq!(masked.literals, ["'it's fine'"]);
    }
    #[test]
    fn enhanced_output_keeps_contractions_intact() {
        let text = "I don't know what it's doing";
        let masked = mask(text, &Snapshot::default());
        assert_eq!(
            validate_enhanced(&masked.text, "I don't know what it's doing", &masked).unwrap(),
            text
        );
    }
    #[test]
    fn mapped_words_inside_quotes_preserve_other_quoted_fillers() {
        let snapshot = Snapshot {
            mappings: vec![mapping("foo", "BAR", None)],
            ..Default::default()
        };
        assert_eq!(prepare("say 'um foo' uh", &snapshot).text, "say 'um BAR'");
    }
    #[test]
    fn accepted_suggestion_can_be_undone_without_losing_manual_edits() {
        let mut s = Snapshot::default();
        s.vocabulary.push(Vocabulary {
            text: "maxGroups".into(),
            ..Default::default()
        });
        let mut result = prepare("use max groups here", &s);
        accept_suggestion(&mut result, 0, 0).unwrap();
        assert_eq!(result.text, "use maxGroups here");
        edit_result(&mut result, "Please use maxGroups here today".into());
        undo_edit(&mut result, 0).unwrap();
        assert_eq!(result.text, "Please use max groups here today");
    }
    #[test]
    fn maps_original_longest_spans_once_and_preserves_exact_output() {
        let s = Snapshot {
            mappings: vec![
                mapping("pod", "POD", None),
                mapping("pod gee see", "podGC", None),
                mapping("podGC", "wrong", None),
            ],
            ..Default::default()
        };
        assert_eq!(
            prepare("POD gee see and pod, tripod", &s).text,
            "podGC and POD, tripod"
        );
    }
    #[test]
    fn auto_accept_applies_sound_alike_suggestions_as_undoable_edits() {
        let mut s = indexed(&["maxGroups", "podGC"]);
        assert_eq!(
            prepare("raise max group and pod gee see", &s).text,
            "raise max group and pod gee see"
        );
        s.auto_accept = true;
        let mut result = prepare("raise max group and pod gee see", &s);
        assert_eq!(result.text, "raise maxGroups and podGC");
        assert!(result.suggestions.is_empty());
        assert_eq!(result.edits.len(), 2);
        assert_eq!(result.basic, "raise max group and pod gee see");
        undo_edit(&mut result, 0).unwrap();
        assert_eq!(result.text, "raise max group and podGC");
    }
    #[test]
    fn auto_accept_leaves_ambiguous_and_conflicting_suggestions_for_review() {
        let mut s = indexed(&["maxGroups", "max_groups"]);
        s.auto_accept = true;
        let result = prepare("raise max groups", &s);
        assert_eq!(result.text, "raise max groups");
        assert_eq!(result.suggestions[0].candidates.len(), 2);
        let mut s = Snapshot {
            mappings: vec![
                mapping("foo", "Repo", Some("a")),
                mapping("foo", "Other", Some("b")),
            ],
            auto_accept: true,
            ..Default::default()
        };
        let result = prepare("foo", &s);
        assert_eq!(result.text, "foo");
        assert!(result.suggestions[0].conflict);
        s.mappings.clear();
        assert!(prepare("foo", &s).edits.is_empty());
    }
    #[test]
    fn auto_accept_applies_an_identifier_declared_in_several_files() {
        let mut s = indexed(&["maxGroups"]);
        s.vocabulary.push(Vocabulary {
            text: "maxGroups".into(),
            repository: "other".into(),
            file: "b.ts".into(),
        });
        s.auto_accept = true;
        let result = prepare("raise max groups", &s);
        assert_eq!(result.text, "raise maxGroups");
        assert_eq!(result.edits.len(), 1);
    }
    #[test]
    fn auto_accept_leaves_overlapping_suggestions_for_review() {
        let mut s = indexed(&["fooBar"]);
        s.mappings = vec![
            mapping("foo bar", "Repo", Some("a")),
            mapping("foo bar", "Other", Some("b")),
        ];
        s.auto_accept = true;
        let result = prepare("use foo bar", &s);
        assert_eq!(result.text, "use foo bar");
        assert!(result.suggestions.iter().any(|s| s.conflict));
        let mut s = indexed(&["maxGroups", "maxGroupsLimit"]);
        s.auto_accept = true;
        let result = prepare("set max groups limit", &s);
        assert_eq!(result.text, "set max groups limit");
        assert_eq!(result.suggestions.len(), 2);
    }
    #[test]
    fn repository_override_and_conflicts_stay_reviewable() {
        let mut s = Snapshot {
            mappings: vec![
                mapping("foo", "global", None),
                mapping("foo", "Repo", Some("a")),
            ],
            ..Default::default()
        };
        assert_eq!(prepare("foo", &s).text, "Repo");
        s.mappings.push(mapping("foo", "Other", Some("b")));
        let r = prepare("foo", &s);
        assert_eq!(r.text, "foo");
        assert!(r.suggestions[0].conflict);
        assert_eq!(r.suggestions[0].candidates.len(), 2);
    }
    #[test]
    fn basic_preserves_quotes_literals_negation_and_repetition() {
        let mut s = Snapshot {
            mappings: vec![mapping("you em", "um", None)],
            ..Default::default()
        };
        s.vocabulary.push(Vocabulary {
            text: "uh.py".into(),
            ..Default::default()
        });
        assert_eq!(
            prepare(
                "um, do not change 12 uh.py or 'uh' or you em uh, repeat repeat",
                &s
            )
            .text,
            "do not change 12 uh.py or 'uh' or um repeat repeat"
        );
        assert_eq!(prepare("um uh", &s).text, "");
        s.cleanup.fillers.clear();
        assert_eq!(prepare("um uh", &s).text, "um uh");
    }
    #[test]
    fn blank_hesitation_tokens_leave_the_transcript_exactly_as_recognized() {
        let mut s = Snapshot::default();
        s.cleanup.fillers = vec!["".into(), "  ".into()];
        assert_eq!(
            prepare(" um,  hello  ,  uh world ", &s).text,
            " um,  hello  ,  uh world "
        );
    }
    #[test]
    fn enhanced_cleanup_is_disabled_unless_settings_name_a_choice() {
        assert_eq!(Cleanup::default().enhanced, "disabled");
        let saved: Cleanup =
            serde_json::from_str(r#"{"mode":"enhanced","provider":"managed"}"#).unwrap();
        assert_eq!(saved.enhanced, "disabled");
        let chosen: Cleanup = serde_json::from_str(r#"{"enhanced":"local"}"#).unwrap();
        assert_eq!(chosen.enhanced, "local");
    }
    fn indexed(texts: &[&str]) -> Snapshot {
        Snapshot {
            vocabulary: texts
                .iter()
                .map(|text| Vocabulary {
                    text: (*text).into(),
                    repository: "r".into(),
                    file: "a.ts".into(),
                })
                .collect(),
            ..Default::default()
        }
    }
    fn suggested(transcript: &str, snapshot: &Snapshot) -> Vec<(String, String)> {
        prepare(transcript, snapshot)
            .suggestions
            .into_iter()
            .map(|s| (s.original, s.candidates[0].text.clone()))
            .collect()
    }
    #[test]
    fn spelled_out_acronym_letters_match_an_indexed_identifier() {
        let s = indexed(&["podGC"]);
        let pair = |original: &str| vec![(original.to_owned(), "podGC".to_owned())];
        assert_eq!(
            suggested("set pod gee see to true", &s),
            pair("pod gee see")
        );
        assert_eq!(suggested("set pod G C to true", &s), pair("pod G C"));
        assert_eq!(suggested("set pod GC to true", &s), pair("pod GC"));
    }
    #[test]
    fn near_miss_of_a_multiword_identifier_is_suggested() {
        let s = indexed(&["maxGroups"]);
        assert_eq!(
            suggested("raise max group to two", &s),
            vec![("max group".to_owned(), "maxGroups".to_owned())]
        );
    }
    #[test]
    fn near_misses_of_plain_words_and_short_terms_are_not_suggested() {
        assert!(suggested("the value is set", &indexed(&["values"])).is_empty());
        assert!(suggested("the eye dee is set", &indexed(&["id"])).is_empty());
        assert!(suggested("tax groups are due", &indexed(&["maxGroups"])).is_empty());
    }
    #[test]
    fn fuzzy_matches_skip_quotes_and_do_not_duplicate_exact_matches() {
        let s = indexed(&["maxGroups"]);
        assert!(suggested("say 'max group' twice", &s).is_empty());
        let result = prepare("raise max groups", &s);
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].candidates.len(), 1);
    }
    #[test]
    fn overlapping_fuzzy_matches_keep_only_the_longest_span() {
        let s = indexed(&["podGC", "podGCInterval"]);
        let got = suggested("set pod gee see to true", &s);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "pod gee see");
    }
    #[test]
    fn recognizer_hint_lists_mapped_text_then_common_identifiers() {
        let mut s = indexed(&[
            "maxGroups",
            "runner",
            "src/app.rs",
            "readFile",
            "maxGroups",
            "podGC",
        ]);
        s.mappings.push(mapping("gee see", "podGC", None));
        let mut off = mapping("old", "unused", None);
        off.enabled = false;
        s.mappings.push(off);
        assert_eq!(
            recognition_hint(&s),
            "Vocabulary: podGC, maxGroups, readFile."
        );
        assert_eq!(recognition_hint(&Snapshot::default()), "");
    }
    #[test]
    fn recognizer_hint_stays_within_its_budget_and_keeps_whole_terms() {
        let names: Vec<_> = (0..300).map(|i| format!("someName{i:03}")).collect();
        let s = indexed(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let hint = recognition_hint(&s);
        assert!(hint.len() <= RECOGNITION_HINT_BYTES);
        assert!(hint.ends_with('.'));
        assert!(hint.contains("someName000, someName001"));
        assert!(!hint.contains("someName299"));
    }
    #[test]
    fn remembered_mapping_replaces_the_heard_phrase_on_the_next_recording() {
        let s = indexed(&["maxGroups"]);
        let result = prepare("raise max group to two", &s);
        let m = mapping_for(&result, 0, 0).unwrap();
        assert_eq!(m.phrase, "max group");
        assert_eq!(m.replacement, "maxGroups");
        assert_eq!(m.repository.as_deref(), Some("r"));
        assert!(m.enabled);
        let next = Snapshot {
            mappings: vec![m],
            ..Default::default()
        };
        assert_eq!(
            prepare("lower max group to one", &next).text,
            "lower maxGroups to one"
        );
    }
    #[test]
    fn mapping_conflicts_and_missing_choices_cannot_be_remembered() {
        let mut s = Snapshot {
            mappings: vec![
                mapping("foo", "Repo", Some("a")),
                mapping("foo", "Other", Some("b")),
            ],
            ..Default::default()
        };
        let result = prepare("foo", &s);
        assert!(mapping_for(&result, 0, 0).is_err());
        s = indexed(&["maxGroups"]);
        let result = prepare("max group", &s);
        assert!(mapping_for(&result, 1, 0).is_err());
        assert!(mapping_for(&result, 0, 1).is_err());
    }
    #[test]
    fn repository_edits_are_only_suggestions() {
        let mut s = Snapshot::default();
        s.vocabulary.push(Vocabulary {
            text: "maxGroups".into(),
            repository: "r".into(),
            file: "a.ts".into(),
        });
        let r = prepare("use max groups and do not change 2", &s);
        assert_eq!(r.text, r.raw);
        assert_eq!(r.suggestions[0].original, "max groups");
        assert_eq!(r.suggestions[0].candidates[0].text, "maxGroups");
    }
}
