//! The recognition languages Conch offers: the whisper.cpp language table at
//! the revision `scripts/build-runtimes.py` pins, in its order.

/// One recognition language: its ISO code and English name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
}
/// The `language` value that asks the recognizer to detect the language.
pub const AUTO: &str = "auto";
pub const ENGLISH: &[&str] = &["en"];
const fn language(code: &'static str, name: &'static str) -> Language {
    Language { code, name }
}
pub const LANGUAGES: [Language; 100] = [
    language("en", "English"),
    language("zh", "Chinese"),
    language("de", "German"),
    language("es", "Spanish"),
    language("ru", "Russian"),
    language("ko", "Korean"),
    language("fr", "French"),
    language("ja", "Japanese"),
    language("pt", "Portuguese"),
    language("tr", "Turkish"),
    language("pl", "Polish"),
    language("ca", "Catalan"),
    language("nl", "Dutch"),
    language("ar", "Arabic"),
    language("sv", "Swedish"),
    language("it", "Italian"),
    language("id", "Indonesian"),
    language("hi", "Hindi"),
    language("fi", "Finnish"),
    language("vi", "Vietnamese"),
    language("he", "Hebrew"),
    language("uk", "Ukrainian"),
    language("el", "Greek"),
    language("ms", "Malay"),
    language("cs", "Czech"),
    language("ro", "Romanian"),
    language("da", "Danish"),
    language("hu", "Hungarian"),
    language("ta", "Tamil"),
    language("no", "Norwegian"),
    language("th", "Thai"),
    language("ur", "Urdu"),
    language("hr", "Croatian"),
    language("bg", "Bulgarian"),
    language("lt", "Lithuanian"),
    language("la", "Latin"),
    language("mi", "Maori"),
    language("ml", "Malayalam"),
    language("cy", "Welsh"),
    language("sk", "Slovak"),
    language("te", "Telugu"),
    language("fa", "Persian"),
    language("lv", "Latvian"),
    language("bn", "Bengali"),
    language("sr", "Serbian"),
    language("az", "Azerbaijani"),
    language("sl", "Slovenian"),
    language("kn", "Kannada"),
    language("et", "Estonian"),
    language("mk", "Macedonian"),
    language("br", "Breton"),
    language("eu", "Basque"),
    language("is", "Icelandic"),
    language("hy", "Armenian"),
    language("ne", "Nepali"),
    language("mn", "Mongolian"),
    language("bs", "Bosnian"),
    language("kk", "Kazakh"),
    language("sq", "Albanian"),
    language("sw", "Swahili"),
    language("gl", "Galician"),
    language("mr", "Marathi"),
    language("pa", "Punjabi"),
    language("si", "Sinhala"),
    language("km", "Khmer"),
    language("sn", "Shona"),
    language("yo", "Yoruba"),
    language("so", "Somali"),
    language("af", "Afrikaans"),
    language("oc", "Occitan"),
    language("ka", "Georgian"),
    language("be", "Belarusian"),
    language("tg", "Tajik"),
    language("sd", "Sindhi"),
    language("gu", "Gujarati"),
    language("am", "Amharic"),
    language("yi", "Yiddish"),
    language("lo", "Lao"),
    language("uz", "Uzbek"),
    language("fo", "Faroese"),
    language("ht", "Haitian Creole"),
    language("ps", "Pashto"),
    language("tk", "Turkmen"),
    language("nn", "Nynorsk"),
    language("mt", "Maltese"),
    language("sa", "Sanskrit"),
    language("lb", "Luxembourgish"),
    language("my", "Myanmar"),
    language("bo", "Tibetan"),
    language("tl", "Tagalog"),
    language("mg", "Malagasy"),
    language("as", "Assamese"),
    language("tt", "Tatar"),
    language("haw", "Hawaiian"),
    language("ln", "Lingala"),
    language("ha", "Hausa"),
    language("ba", "Bashkir"),
    language("jw", "Javanese"),
    language("su", "Sundanese"),
    language("yue", "Cantonese"),
];
/// The first `N` codes of `LANGUAGES`.
const fn first_codes<const N: usize>() -> [&'static str; N] {
    let mut codes = [""; N];
    let mut i = 0;
    while i < N {
        codes[i] = LANGUAGES[i].code;
        i += 1;
    }
    codes
}
/// Every language, for the models with 100 language tokens (`n_vocab` 51866).
pub const CODES: [&str; LANGUAGES.len()] = first_codes();
/// Every language but Cantonese, the last in the table. Models with 99 language
/// tokens (`n_vocab` 51865) cannot select it: its id resolves to the translate token.
pub const CODES_99: [&str; LANGUAGES.len() - 1] = first_codes();
/// Whether `code` is in the table or is `auto`.
pub fn known(code: &str) -> bool {
    code == AUTO || LANGUAGES.iter().any(|language| language.code == code)
}
/// The code of the language whisper.cpp reports by its lower-case English name.
pub fn code_for_name(name: &str) -> Option<&'static str> {
    let name = name.to_lowercase();
    LANGUAGES
        .iter()
        .find(|language| language.name.to_lowercase() == name)
        .map(|language| language.code)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whisper_reports_a_language_by_its_lower_case_name() {
        assert_eq!(code_for_name("french"), Some("fr"));
        assert_eq!(code_for_name("haitian creole"), Some("ht"));
        assert_eq!(code_for_name("cantonese"), Some("yue"));
        assert_eq!(code_for_name("English"), Some("en"));
        assert_eq!(code_for_name("klingon"), None);
        assert_eq!(code_for_name(""), None);
        for language in LANGUAGES {
            assert_eq!(
                code_for_name(&language.name.to_lowercase()),
                Some(language.code)
            );
        }
    }
}
