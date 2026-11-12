import type { Language, ModelView } from "./types";

/** A short slice of the Rust language table, in its (whisper.cpp) order. */
export const LANGUAGES: Language[] = [
  { code: "en", name: "English" },
  { code: "zh", name: "Chinese" },
  { code: "de", name: "German" },
  { code: "fr", name: "French" },
  { code: "yue", name: "Cantonese" },
];
const CODES = LANGUAGES.map((language) => language.code);
/** Tiny, Base, and Small have 99 languages: every one but Cantonese, the last in the table. */
const CODES_99 = CODES.filter((code) => code !== "yue");

/** The catalog `view` returns, as the Rust catalog lists it. `installed` names the ids on disk. */
export const catalog = (installed: string[] = []): ModelView[] =>
  (
    [
      {
        id: "tiny.en",
        label: "Tiny (English)",
        note: "Fastest",
        size: 77704715,
        engine: "whisper",
        languages: ["en"],
      },
      {
        id: "base.en",
        label: "Base (English)",
        note: "Balanced",
        size: 147964211,
        engine: "whisper",
        languages: ["en"],
      },
      {
        id: "small.en",
        label: "Small (English)",
        note: "Most accurate",
        size: 487614201,
        engine: "whisper",
        languages: ["en"],
      },
      {
        id: "tiny",
        label: "Tiny (multilingual)",
        note: "Fastest",
        size: 77691713,
        engine: "whisper",
        languages: CODES_99,
      },
      {
        id: "base",
        label: "Base (multilingual)",
        note: "Balanced",
        size: 147951465,
        engine: "whisper",
        languages: CODES_99,
      },
      {
        id: "small",
        label: "Small (multilingual)",
        note: "Accurate",
        size: 487601967,
        engine: "whisper",
        languages: CODES_99,
      },
      {
        id: "large-v3-turbo",
        label: "Large Turbo (multilingual)",
        note: "Most accurate",
        size: 574041195,
        engine: "whisper",
        languages: CODES,
      },
      {
        id: "silero-vad",
        label: "Voice detection (Silero)",
        note: "",
        size: 885098,
        engine: null,
        languages: [],
      },
      {
        id: "s1-mini",
        label: "S1-mini by Superwhisper",
        note: "",
        size: 484219808,
        engine: "llama",
        languages: ["en"],
      },
    ] satisfies Omit<ModelView, "ready">[]
  ).map((model) => ({ ...model, ready: installed.includes(model.id) }));
