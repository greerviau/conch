//! Case folding and word characters shared by mapping, identifier, and cleanup matching.

use regex_syntax::hir::{ClassUnicode, ClassUnicodeRange};
use std::{
    collections::HashMap,
    sync::{LazyLock, RwLock},
};

/// Folds one character the way the regex crate's `(?i)` folds it (Unicode simple case
/// folding): every case variant of a character maps to one representative, the smallest
/// lower-case variant or, when none is lower case, the smallest variant. A phrase compares
/// equal to exactly the text its pattern matches. The tables are the regex crate's own, so
/// the two cannot disagree: the Greek capital sigma (U+03A3), small sigma (U+03C3), and final
/// sigma (U+03C2) fold together, as do the German sharp s (U+00DF) and its capital (U+1E9E),
/// while the dotless i (U+0131) stays apart from `i`.
pub fn fold_char(c: char) -> char {
    static NON_ASCII: LazyLock<RwLock<HashMap<char, char>>> = LazyLock::new(Default::default);
    if c.is_ascii() {
        // The smallest lower-case variant of an ASCII letter is its small form.
        return c.to_ascii_lowercase();
    }
    if let Some(&folded) = NON_ASCII.read().unwrap().get(&c) {
        return folded;
    }
    let mut class = ClassUnicode::new([ClassUnicodeRange::new(c, c)]);
    class.case_fold_simple();
    let members = || class.iter().flat_map(|range| range.start()..=range.end());
    let folded = members()
        .find(|m| m.is_lowercase())
        .or_else(|| members().next())
        .unwrap_or(c);
    NON_ASCII.write().unwrap().insert(c, folded);
    folded
}
/// Folds a string by character.
pub fn fold(text: &str) -> String {
    text.chars().map(fold_char).collect()
}
/// Whether `c` is a combining mark, which attaches to the letter before it.
pub fn is_mark(c: char) -> bool {
    static MARK: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"^\p{M}$").unwrap());
    !c.is_ascii() && MARK.is_match(c.encode_utf8(&mut [0; 4]))
}
/// Whether `c` belongs to a word: a letter, a digit, an underscore, or a combining mark.
pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || is_mark(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn every_char() -> impl Iterator<Item = char> {
        (0..=0x10FFFFu32).filter_map(char::from_u32)
    }
    #[test]
    fn folding_puts_exactly_the_characters_the_regex_crate_treats_as_case_variants_together() {
        let matches = |pattern: char, other: char| {
            regex::Regex::new(&format!("(?i)^{}$", regex::escape(&pattern.to_string())))
                .unwrap()
                .is_match(&other.to_string())
        };
        let mut classes = BTreeMap::<char, Vec<char>>::new();
        for c in every_char().filter(|c| c.is_alphabetic()) {
            classes.entry(fold_char(c)).or_default().push(c);
        }
        let mut merged_too_much = vec![];
        for members in classes.values().filter(|m| m.len() > 1) {
            for other in &members[1..] {
                if !matches(members[0], *other) {
                    merged_too_much.push((members[0], *other));
                }
            }
        }
        assert_eq!(
            merged_too_much,
            vec![],
            "folded together but not regex variants"
        );
        // Non-ASCII folding is the regex crate's own table. The ASCII shortcut is ours, so
        // check it against every cased character.
        let cased: Vec<char> = every_char()
            .filter(|c| c.is_lowercase() || c.is_uppercase())
            .collect();
        let mut kept_apart = vec![];
        for c in (0..128u8).map(char::from) {
            let re =
                regex::Regex::new(&format!("(?i)^{}$", regex::escape(&c.to_string()))).unwrap();
            for d in &cased {
                if re.is_match(&d.to_string()) != (fold_char(c) == fold_char(*d)) {
                    kept_apart.push((c, *d));
                }
            }
        }
        assert_eq!(
            kept_apart,
            vec![],
            "ASCII folding disagrees with the regex crate"
        );
    }
    #[test]
    fn greek_sigmas_and_german_sharp_s_fold_with_their_case_variants() {
        assert_eq!(fold("\u{3A3}"), fold("\u{3C3}"));
        assert_eq!(fold("\u{3C2}"), fold("\u{3C3}"));
        assert_eq!(fold("\u{1E9E}"), fold("\u{DF}"));
        assert_eq!(fold("Stra\u{DF}e"), fold("STRA\u{1E9E}E"));
        assert_ne!(fold("\u{131}"), fold("i"));
        assert_eq!(fold("\u{212A}"), fold("k"));
        assert_eq!(fold("PodGC"), fold("podgc"));
        assert_ne!(fold("Stra\u{DF}e"), fold("STRASSE"));
    }
    #[test]
    fn combining_marks_belong_to_the_word_they_follow() {
        assert!(is_word('a') && is_word('7') && is_word('_') && is_word('\u{DF}'));
        assert!(is_word('\u{301}'));
        assert!(is_word('\u{307}'));
        assert!(!is_word(' ') && !is_word('-') && !is_word('.'));
    }
}
