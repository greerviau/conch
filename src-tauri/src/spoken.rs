//! Sound-alike matching between indexed identifiers and recognizer output.
//!
//! A recognizer writes `podGC` as "pod gee see", "pod G C" or "pod GC", and
//! writes `maxGroups` as "max group". Each indexed term gets one or two
//! sound keys, the lowercase words joined without spaces. All-uppercase words
//! also get a second key with each letter spelled by its name. Every run of
//! up to `MAX_WORDS` transcript words is compared with the keys that start with
//! the same three letters and have a similar length. Terms made of several words allow a small edit distance.
//! A term of one word must match exactly.
use crate::core::Vocabulary;
use std::collections::HashMap;

const LETTER_NAMES: [&str; 26] = [
    "ay",
    "bee",
    "see",
    "dee",
    "ee",
    "eff",
    "gee",
    "aitch",
    "eye",
    "jay",
    "kay",
    "el",
    "em",
    "en",
    "oh",
    "pee",
    "queue",
    "are",
    "ess",
    "tee",
    "you",
    "vee",
    "doubleyou",
    "ex",
    "why",
    "zee",
];
/// Keys shorter than this match too many ordinary words.
const MIN_KEY: usize = 4;
/// Longest run of transcript words compared with one key.
const MAX_WORDS: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub start: usize,
    pub end: usize,
    pub distance: usize,
    /// Index into the vocabulary passed to `find`.
    pub term: usize,
}

struct Entry {
    key: Vec<char>,
    term: usize,
    tolerance: usize,
}

struct Token {
    start: usize,
    end: usize,
    lower: String,
}

/// Splits `term` at non-alphanumeric characters and before an uppercase
/// letter that follows a lowercase letter or digit.
fn words(term: &str) -> Vec<String> {
    let mut words = vec![];
    let mut word = String::new();
    let mut previous = None::<char>;
    for c in term.chars() {
        if !c.is_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            previous = None;
            continue;
        }
        let lower_to_upper = c.is_ascii_uppercase()
            && previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit());
        if lower_to_upper && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        word.push(c);
        previous = Some(c);
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

fn is_acronym(word: &str) -> bool {
    word.len() >= 2 && word.bytes().all(|b| b.is_ascii_uppercase())
}

/// Whether `term` is written as a multi-word identifier or an acronym, the
/// spellings a recognizer gets wrong.
pub fn is_identifier(term: &str) -> bool {
    if term
        .chars()
        .any(|c| !(c.is_alphanumeric() || c == '_' || c == '-'))
    {
        return false;
    }
    let words = words(term);
    words.len() >= 2 || (words.len() == 1 && is_acronym(&words[0]))
}

fn keys(words: &[String]) -> Vec<String> {
    let plain: String = words.iter().map(|w| w.to_lowercase()).collect();
    let named: String = words
        .iter()
        .map(|w| {
            if is_acronym(w) {
                w.bytes()
                    .map(|b| LETTER_NAMES[(b - b'A') as usize])
                    .collect()
            } else {
                w.to_lowercase()
            }
        })
        .collect();
    let mut keys = vec![plain];
    if named != keys[0] {
        keys.push(named);
    }
    keys.retain(|k| k.chars().count() >= MIN_KEY);
    keys
}

/// Largest value `tolerance` returns.
const MAX_TOLERANCE: usize = 2;

fn tolerance(length: usize) -> usize {
    match length {
        0..=5 => 0,
        6..=11 => 1,
        _ => MAX_TOLERANCE,
    }
}

fn tokens(text: &str) -> Vec<Token> {
    let mut tokens = vec![];
    let mut open: Option<usize> = None;
    for (at, c) in text.char_indices() {
        match (c.is_alphanumeric(), open) {
            (true, None) => open = Some(at),
            (false, Some(start)) => {
                tokens.push(Token {
                    start,
                    end: at,
                    lower: text[start..at].to_lowercase(),
                });
                open = None;
            }
            _ => {}
        }
    }
    if let Some(start) = open {
        tokens.push(Token {
            start,
            end: text.len(),
            lower: text[start..].to_lowercase(),
        });
    }
    tokens
}

/// Edit distance between `a` and `b` when it is at most `max`.
fn within(a: &[char], b: &[char], max: usize) -> Option<usize> {
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut row = vec![0; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let substitute = previous[j] + usize::from(x != y);
            row[j + 1] = substitute.min(previous[j + 1] + 1).min(row[j] + 1);
        }
        if row.iter().all(|&d| d > max) {
            return None;
        }
        std::mem::swap(&mut previous, &mut row);
    }
    previous.last().copied().filter(|&d| d <= max)
}

/// Finds runs of transcript words that sound like an indexed term but are not
/// written like it. Runs that overlap a range in `blocked` are skipped.
pub fn find(text: &str, vocabulary: &[Vocabulary], blocked: &[(usize, usize)]) -> Vec<Match> {
    let tokens = tokens(text);
    if tokens.is_empty() {
        return vec![];
    }
    let mut index = HashMap::<[char; 3], Vec<Entry>>::new();
    for (term, v) in vocabulary.iter().enumerate() {
        let words = words(&v.text);
        if words.is_empty() || words.len() > MAX_WORDS {
            continue;
        }
        let structured = words.len() >= 2;
        for key in keys(&words) {
            let key: Vec<char> = key.chars().collect();
            let tolerance = if structured { tolerance(key.len()) } else { 0 };
            index
                .entry([key[0], key[1], key[2]])
                .or_default()
                .push(Entry {
                    key,
                    term,
                    tolerance,
                });
        }
    }
    for entries in index.values_mut() {
        entries.sort_by_key(|e| e.key.len());
    }
    let mut found = vec![];
    for size in 1..=MAX_WORDS.min(tokens.len()) {
        for run in tokens.windows(size) {
            let (start, end) = (run[0].start, run[size - 1].end);
            if blocked.iter().any(|&(s, e)| start < e && s < end) {
                continue;
            }
            let key: Vec<char> = run.iter().flat_map(|t| t.lower.chars()).collect();
            if key.len() < MIN_KEY {
                continue;
            }
            let Some(entries) = index.get(&[key[0], key[1], key[2]]) else {
                continue;
            };
            let first = entries.partition_point(|e| e.key.len() + MAX_TOLERANCE < key.len());
            for entry in entries[first..]
                .iter()
                .take_while(|e| e.key.len() <= key.len() + MAX_TOLERANCE)
            {
                let Some(distance) = within(&key, &entry.key, entry.tolerance) else {
                    continue;
                };
                if text[start..end].eq_ignore_ascii_case(&vocabulary[entry.term].text) {
                    continue;
                }
                found.push(Match {
                    start,
                    end,
                    distance,
                    term: entry.term,
                });
            }
        }
    }
    found.sort_by_key(|m| (m.start, m.end, m.term, m.distance));
    found.dedup_by_key(|m| (m.start, m.end, m.term));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    fn term(text: &str) -> Vocabulary {
        Vocabulary {
            text: text.into(),
            ..Default::default()
        }
    }
    #[test]
    fn keys_spell_out_uppercase_words_and_drop_short_keys() {
        assert_eq!(
            keys(&words("podGC")),
            vec!["podgc".to_owned(), "podgeesee".to_owned()]
        );
        assert_eq!(keys(&words("max_groups")), vec!["maxgroups".to_owned()]);
        assert!(keys(&words("id")).is_empty());
    }
    #[test]
    fn edit_distance_stops_above_the_limit() {
        let chars = |s: &str| s.chars().collect::<Vec<_>>();
        assert_eq!(within(&chars("maxgroup"), &chars("maxgroups"), 1), Some(1));
        assert_eq!(within(&chars("maxgrou"), &chars("maxgroups"), 1), None);
        assert_eq!(within(&chars("same"), &chars("same"), 0), Some(0));
    }
    #[test]
    fn runs_covering_other_text_are_not_matched() {
        let vocabulary = [term("podGC")];
        assert!(find("so pod gee see to", &vocabulary, &[(0, 2)]).len() == 1);
        assert!(find("so pod gee see to", &vocabulary, &[(3, 6)]).is_empty());
    }
}
