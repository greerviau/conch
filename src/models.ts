import type { Language, ModelView } from "./types";

/** Download size as shown beside a model. */
export const size = (model: Pick<ModelView, "size">) =>
  `${(model.size / 1e6).toFixed(1)} MB`;
/** Models that turn audio into text, in the order Rust lists them (smallest first). */
export const recognizers = (models: ModelView[]) =>
  models.filter(
    (model) => model.engine === "whisper" || model.engine === "transcribe",
  );
const required = (model: ModelView | undefined) => {
  if (!model) throw new Error("The model catalog is incomplete");
  return model;
};
/** The Silero model every recognizer loads. It has no engine of its own. */
export const voiceDetection = (models: ModelView[]) =>
  required(models.find((model) => model.engine === null));
/** The managed Enhanced cleanup model. */
export const cleanupModel = (models: ModelView[]) =>
  required(models.find((model) => model.engine === "llama"));
/** The recognition model with this id, or the first one when the id is unknown. */
export const recognizerInfo = (models: ModelView[], id: string) => {
  const all = recognizers(models);
  return required(all.find((model) => model.id === id) ?? all[0]);
};
/** Whether the model recognizes the language. Auto-detect needs more than one. */
export const supports = (
  model: Pick<ModelView, "languages">,
  language: string,
) =>
  language === "auto"
    ? model.languages.length > 1
    : model.languages.includes(language);
/** Whether Settings offers the model for the language. English offers the English-only models in place of their multilingual variants. */
export const offered = (
  model: Pick<ModelView, "languages" | "english_variant">,
  language: string,
) =>
  supports(model, language) &&
  !(language === "en" && model.english_variant !== null);
/** The recognition models offered for the language, in catalog order. */
export const recognizersFor = (models: ModelView[], language: string) =>
  recognizers(models).filter((model) => offered(model, language));
/** English first, then the other languages by name. */
export const languageMenu = (languages: Language[]) => [
  ...languages.filter((language) => language.code === "en"),
  ...languages
    .filter((language) => language.code !== "en")
    .sort((a, b) => a.name.localeCompare(b.name, "en")),
];
/**
 * The model to use after the language changes: the current one if it is
 * offered for the language, its English-only variant for English, otherwise
 * Small (Small (English) for English).
 */
export function modelForLanguage(
  models: ModelView[],
  language: string,
  current: string,
) {
  const options = recognizersFor(models, language);
  if (options.some((model) => model.id === current)) return current;
  const variant =
    language === "en"
      ? models.find((model) => model.id === current)?.english_variant
      : null;
  if (variant && options.some((model) => model.id === variant)) return variant;
  const preferred = language === "en" ? "small.en" : "small";
  return (
    (options.find((model) => model.id === preferred) ?? options[0])?.id ??
    current
  );
}
