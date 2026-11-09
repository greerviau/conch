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
    pub mode: String,
    pub fillers: Vec<String>,
    pub excluded: Vec<String>,
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub remote_enabled: bool,
}
impl Default for Cleanup {
    fn default() -> Self {
        Self {
            mode: "basic".into(),
            fillers: vec!["um".into(), "uh".into()],
            excluded: vec![],
            provider: "managed".into(),
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
    let quotes = regex::Regex::new(r#""[^"\n]*"|'[^'\n]*'|`[^`\n]*`"#).unwrap();
    let mut protected: Vec<_> = quotes
        .find_iter(raw)
        .map(|m| (m.start(), m.end(), m.as_str().to_owned()))
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
        let token = out.token(&literal);
        out.text.push_str(&token);
        at = end;
    }
    out.text.push_str(&raw[at..]);
    out
}
pub fn basic(masked: &str, cleanup: &Cleanup) -> String {
    if cleanup.mode == "off" {
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
    let camel = regex::Regex::new(r"([a-z0-9])([A-Z])").unwrap();
    camel
        .replace_all(text, "$1 $2")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
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
    for v in &snapshot.vocabulary {
        let phrase = spoken(&v.text);
        if phrase == v.text.to_lowercase() {
            continue;
        }
        if let Some(re) = phrase_regex(&phrase) {
            for m in re.find_iter(text).filter(|m| {
                boundary(text, m.start(), m.end())
                    && !protected.literals.iter().any(|s| s.contains(m.as_str()))
            }) {
                groups
                    .entry((m.start(), m.end(), false))
                    .or_default()
                    .push(v.clone());
            }
        }
    }
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
pub fn prepare(raw: &str, snapshot: &Snapshot) -> ResultText {
    let masked = mask(raw, snapshot);
    let text = masked
        .restore(&basic(&masked.text, &snapshot.cleanup))
        .unwrap_or_else(|| raw.into());
    ResultText {
        raw: raw.into(),
        basic: text.clone(),
        suggestions: suggestions(&text, snapshot),
        text,
        edits: vec![],
        status: "Prepared locally".into(),
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
    let mut text = result.text.clone();
    text.replace_range(suggestion.start..suggestion.end, &replacement);
    result.suggestions.remove(index);
    edit_result(result, text);
    result.edits.push(AppliedEdit {
        start: suggestion.start,
        end: suggestion.start + replacement.len(),
        original: suggestion.original,
        replacement,
    });
    Ok(())
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
    let mut text = result.text.clone();
    text.replace_range(edit.start..edit.end, &edit.original);
    result.edits.remove(index);
    edit_result(result, text);
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
        s.cleanup.mode = "off".into();
        assert_eq!(prepare("um uh", &s).text, "um uh");
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
