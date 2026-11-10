export type ModelInfo = { id: string; label: string; mb: number };
/** English recognizers from smallest to most accurate. Ids match the Rust manifest. */
export const RECOGNIZERS: (ModelInfo & { note: string })[] = [
  { id: "tiny.en", label: "Tiny (English)", mb: 77.7, note: "Fastest" },
  { id: "base.en", label: "Base (English)", mb: 148.0, note: "Balanced" },
  {
    id: "small.en",
    label: "Small (English)",
    mb: 487.6,
    note: "Most accurate",
  },
];
export const VAD: ModelInfo = {
  id: "silero-vad",
  label: "Voice detection (Silero)",
  mb: 0.9,
};
export const CLEANUP: ModelInfo = {
  id: "s1-mini",
  label: "S1-mini by Superwhisper",
  mb: 484.2,
};
export const MODELS: ModelInfo[] = [...RECOGNIZERS, VAD, CLEANUP];
export const size = (model: ModelInfo) => `${model.mb.toFixed(1)} MB`;
