//! Sound-alike matching between indexed identifiers and recognizer output.
//!
//! A recognizer writes `podGC` as "pod gee see", "pod G C" or "pod GC", and
//! writes `maxGroups` as "max group". Each indexed term gets one or two
//! sound keys, the lowercase words joined without spaces. All-uppercase words
//! also get a second key with each letter spelled by its name. Every run of
//! up to `MAX_WORDS` transcript words is compared with the keys that start with
//! the same three letters and have a similar length. A term of several words allows a small edit
//! distance when its key has at least eight letters and the run has as many words as the term.
//! Shorter keys and shorter runs must match exactly, because one wrong letter in a short key
//! usually spells another word. A term of one word must match exactly.
use crate::{
    core::Vocabulary,
    fold::{fold, is_mark},
};
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
/// How English names the letters. Only English has a table; the sound of a letter in
/// another language is not known.
fn letter_names(language: &str) -> Option<&'static [&'static str; 26]> {
    (language == "en").then_some(&LETTER_NAMES)
}
/// The form two spellings are compared in: lower case, `ss` for the German sharp s, and
/// every case variant of a letter folded together (all three Greek sigmas, for one).
fn sound_key(text: &str) -> String {
    fold(&text.to_lowercase().replace('\u{DF}', "ss"))
}
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
    /// Words of the identifier. A run that is off by a letter must cover this many transcript
    /// words, because a single word one letter away from a short identifier is usually another
    /// word ("appear" for `AppBar`).
    words: usize,
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
        if !(c.is_alphanumeric() || is_mark(c)) {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            previous = None;
            continue;
        }
        let lower_to_upper = c.is_uppercase()
            && previous.is_some_and(|p| p.is_lowercase() || p.is_numeric() || is_mark(p));
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
    word.chars().count() >= 2 && word.chars().all(|c| c.is_uppercase())
}

/// Whether `term` is written as a multi-word identifier or an acronym, the
/// spellings a recognizer gets wrong.
pub fn is_identifier(term: &str) -> bool {
    if term
        .chars()
        .any(|c| !(c.is_alphanumeric() || is_mark(c) || c == '_' || c == '-'))
    {
        return false;
    }
    let words = words(term);
    words.len() >= 2 || (words.len() == 1 && is_acronym(&words[0]))
}

fn keys(words: &[String], letters: Option<&[&str; 26]>) -> Vec<String> {
    let plain: String = words.iter().map(|w| sound_key(w)).collect();
    let named: String = words
        .iter()
        .map(|w| match letters {
            Some(names) if is_acronym(w) && w.bytes().all(|b| b.is_ascii_uppercase()) => {
                w.bytes().map(|b| names[(b - b'A') as usize]).collect()
            }
            _ => sound_key(w),
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
        0..=7 => 0,
        8..=11 => 1,
        _ => MAX_TOLERANCE,
    }
}

fn tokens(text: &str) -> Vec<Token> {
    let mut tokens = vec![];
    let mut open: Option<usize> = None;
    for (at, c) in text.char_indices() {
        match (c.is_alphanumeric() || is_mark(c), open) {
            (true, None) => open = Some(at),
            (false, Some(start)) => {
                tokens.push(Token {
                    start,
                    end: at,
                    lower: sound_key(&text[start..at]),
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
            lower: sound_key(&text[start..]),
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
pub fn find(
    text: &str,
    vocabulary: &[Vocabulary],
    blocked: &[(usize, usize)],
    language: &str,
) -> Vec<Match> {
    let letters = letter_names(language);
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
        for key in keys(&words, letters) {
            let key: Vec<char> = key.chars().collect();
            let tolerance = if structured { tolerance(key.len()) } else { 0 };
            index
                .entry([key[0], key[1], key[2]])
                .or_default()
                .push(Entry {
                    key,
                    term,
                    tolerance,
                    words: words.len(),
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
                if distance > 0 && size < entry.words {
                    continue;
                }
                if fold(&text[start..end]) == fold(&vocabulary[entry.term].text) {
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
            keys(&words("podGC"), Some(&LETTER_NAMES)),
            vec!["podgc".to_owned(), "podgeesee".to_owned()]
        );
        assert_eq!(
            keys(&words("max_groups"), Some(&LETTER_NAMES)),
            vec!["maxgroups".to_owned()]
        );
        assert!(keys(&words("id"), Some(&LETTER_NAMES)).is_empty());
    }
    #[test]
    fn edit_distance_stops_above_the_limit() {
        let chars = |s: &str| s.chars().collect::<Vec<_>>();
        assert_eq!(within(&chars("maxgroup"), &chars("maxgroups"), 1), Some(1));
        assert_eq!(within(&chars("maxgrou"), &chars("maxgroups"), 1), None);
        assert_eq!(within(&chars("same"), &chars("same"), 0), Some(0));
    }
    #[test]
    fn an_ordinary_word_one_letter_from_a_short_identifier_is_not_matched() {
        let vocabulary = [term("AppBar")];
        assert!(find("the buttons appear here", &vocabulary, &[], "en").is_empty());
        assert!(find("an app bear", &vocabulary, &[], "en").is_empty());
        assert_eq!(find("an app bar", &vocabulary, &[], "en").len(), 1);
    }
    #[test]
    fn a_misspelling_must_cover_as_many_words_as_the_identifier_has() {
        let vocabulary = [term("maxGroups")];
        assert!(find("raise maxgroup now", &vocabulary, &[], "en").is_empty());
        assert_eq!(find("raise max group now", &vocabulary, &[], "en").len(), 1);
        assert_eq!(
            find("raise max groups now", &vocabulary, &[], "en").len(),
            1
        );
    }
    #[test]
    fn runs_covering_other_text_are_not_matched() {
        let vocabulary = [term("podGC")];
        assert!(find("so pod gee see to", &vocabulary, &[(0, 2)], "en").len() == 1);
        assert!(find("so pod gee see to", &vocabulary, &[(3, 6)], "en").is_empty());
    }
    #[test]
    fn spelled_letters_match_an_acronym_in_english_only() {
        let vocabulary = [term("podGC")];
        assert_eq!(find("so pod gee see to", &vocabulary, &[], "en").len(), 1);
        for language in ["de", "fr", "pt", "auto"] {
            assert!(
                find("so pod gee see to", &vocabulary, &[], language).is_empty(),
                "{language}"
            );
        }
        // Letters written as capitals need no letter-name table.
        for language in ["en", "de", "auto"] {
            assert_eq!(
                find("so pod G C to", &vocabulary, &[], language).len(),
                1,
                "{language}"
            );
        }
    }
    #[test]
    fn identifier_words_split_before_any_uppercase_letter_that_follows_a_lowercase_one() {
        assert_eq!(words("grünÄpfel"), ["grün", "Äpfel"]);
        assert_eq!(words("straßeÖl2Ärger"), ["straße", "Öl2", "Ärger"]);
        assert_eq!(words("podGC"), ["pod", "GC"]);
    }
    #[test]
    fn an_all_uppercase_word_of_any_script_is_an_acronym() {
        assert!(is_identifier("ÖPNV"));
        assert!(is_identifier("GC"));
        assert!(!is_identifier("Öpnv"));
        assert!(!is_identifier("Ö"));
    }
    #[test]
    fn a_word_that_differs_from_the_identifier_only_in_case_is_not_a_sound_alike() {
        let vocabulary = [term("\u{DC}berGr\u{F6}\u{DF}e")];
        assert!(find("die \u{FC}bergr\u{F6}\u{DF}e hier", &vocabulary, &[], "de").is_empty());
        assert!(
            find(
                "die \u{DC}BERGR\u{D6}\u{1E9E}E hier",
                &vocabulary,
                &[],
                "de"
            )
            .is_empty()
        );
        assert_eq!(
            find("die \u{FC}ber gr\u{F6}\u{DF}e hier", &vocabulary, &[], "de").len(),
            1
        );
    }
    #[test]
    fn sound_alike_keys_treat_sharp_s_as_ss_and_all_sigmas_as_one_letter() {
        let vocabulary = [term("Stra\u{DF}e\u{D6}l")];
        assert_eq!(
            find("die STRASSE \u{D6}L hier", &vocabulary, &[], "de").len(),
            1
        );
        assert_eq!(
            find("die strasse \u{F6}l hier", &vocabulary, &[], "auto").len(),
            1
        );
        let greek = [term(
            "\u{3BF}\u{3B4}\u{3CC}\u{3C2}\u{3A0}\u{3AF}\u{3BD}\u{3B1}\u{3BA}\u{3B1}\u{3C2}",
        )];
        assert_eq!(
            find(
                "\u{3BF}\u{3B4}\u{3CC}\u{3A3} \u{3C0}\u{3AF}\u{3BD}\u{3B1}\u{3BA}\u{3B1}\u{3A3}",
                &greek,
                &[],
                "auto"
            )
            .len(),
            1
        );
    }
    #[test]
    fn a_combining_mark_stays_with_the_letter_it_follows() {
        // "\u{130}" lower-cases to "i" and a combining dot, which a recognizer may write as two characters.
        let vocabulary = [term("\u{130}Value")];
        assert_eq!(
            find("an i\u{307} value here", &vocabulary, &[], "tr").len(),
            1
        );
        assert_eq!(words("cafe\u{301}Latte"), ["cafe\u{301}", "Latte"]);
        assert!(is_identifier("cafe\u{301}Latte"));
        let tokens = tokens("a cafe\u{301} b");
        assert_eq!(tokens.len(), 3);
        assert_eq!(
            &"a cafe\u{301} b"[tokens[1].start..tokens[1].end],
            "cafe\u{301}"
        );
    }
}
