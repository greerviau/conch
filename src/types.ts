export type Repository = {
  id: string;
  root: string;
  exclusions: string[];
};
export type Mapping = {
  phrase: string;
  replacement: string;
  repository: string | null;
  enabled: boolean;
};
export type Cleanup = {
  /** Hesitation tokens by language code. */
  fillers: Record<string, string[]>;
  excluded: string[];
  /** The endpoint local and remote Enhanced cleanup use, shared by every profile. */
  endpoint: string;
  model: string;
  remote_enabled: boolean;
};
/** The recognition language and model, Enhanced cleanup, and repository context of a recording. */
export type Profile = {
  id: string;
  name: string;
  /** ISO code from the language table, or `auto`. */
  language: string;
  recognizer: string;
  /** Enhanced cleanup: disabled, managed, local, or remote. */
  enhanced: string;
  /** Ids of the repositories that supply context. */
  repositories: string[];
  auto_accept: boolean;
};
export type Settings = {
  repositories: Repository[];
  mappings: Mapping[];
  cleanup: Cleanup;
  hotkey: string;
  activation: string;
  auto_paste: boolean;
  keep_history: boolean;
  /** Input device name. Empty follows the system input. */
  microphone: string;
  show_overlay: boolean;
  check_updates: boolean;
  /** Never empty. */
  profiles: Profile[];
  active_profile: string;
};
/** The profile a recording uses: the active one, or the first when the id is unknown. */
export const activeProfile = (settings: Settings) =>
  settings.profiles.find((p) => p.id === settings.active_profile) ??
  settings.profiles[0];
/** `settings` with the profile `id` changed by `patch`. */
export const withProfile = (
  settings: Settings,
  id: string,
  patch: Partial<Profile>,
): Settings => ({
  ...settings,
  profiles: settings.profiles.map((p) =>
    p.id === id ? { ...p, ...patch } : p,
  ),
});
/** `settings` with the active profile changed by `patch`. */
export const withActive = (settings: Settings, patch: Partial<Profile>) =>
  withProfile(settings, activeProfile(settings).id, patch);
/**
 * `settings` with `repositories` as the repository list. A repository that is
 * new supplies context to the active profile, and a removed one leaves every profile.
 */
export const withRepositories = (
  settings: Settings,
  repositories: Repository[],
): Settings => {
  const added = repositories
    .filter((r) => !settings.repositories.some((old) => old.id === r.id))
    .map((r) => r.id);
  const kept = new Set(repositories.map((r) => r.id));
  const active = activeProfile(settings).id;
  return {
    ...settings,
    repositories,
    profiles: settings.profiles.map((p) => ({
      ...p,
      repositories: [
        ...p.repositories.filter((id) => kept.has(id)),
        ...(p.id === active ? added : []),
      ],
    })),
  };
};
export type Candidate = { text: string; repository: string; file: string };
export type ResultText = {
  raw: string;
  text: string;
  basic: string;
  status: string;
  edits: { original: string; replacement: string }[];
  suggestions: {
    original: string;
    candidates: Candidate[];
    conflict: boolean;
  }[];
};
export type IndexStatus = {
  available: boolean;
  incomplete: boolean;
  skipped: number;
  entries: number;
};
/** Repository id, index state, and the Unix time of the last refresh. */
export type IndexRow = [string, IndexStatus, number];
export type Term = { text: string; repository: string; file: string };
export type TermPage = { items: Term[]; total: number };
export type HistoryEntry = {
  id: number;
  /** Unix time the result was delivered. */
  created: number;
  text: string;
  raw: string;
};
export type HistoryPage = { items: HistoryEntry[]; total: number };
export type LogEntry = {
  /** Unix time in milliseconds. */
  time: number;
  level: "info" | "warn" | "error";
  source: string;
  message: string;
  detail?: string;
};
export type LogPage = {
  items: LogEntry[];
  /** Entries that match the filters. */
  total: number;
  /** Entries in the log before filtering. */
  all: number;
  /** Every source that has an entry, sorted. */
  sources: string[];
  /** Size of the log files on disk. */
  bytes: number;
};
/** A model the catalog offers, with whether it is on disk. Engine is null for voice detection. */
export type ModelView = {
  id: string;
  label: string;
  note: string;
  /** Bytes. */
  size: number;
  engine: "whisper" | "transcribe" | "llama" | null;
  /** ISO codes of the languages the model recognizes. */
  languages: string[];
  ready: boolean;
};
export type Language = { code: string; name: string };
export type View = {
  settings: Settings;
  phase: string;
  result: ResultText | null;
  error: string;
  indexes: IndexRow[];
  /** The id of the newest history entry, or 0. */
  history_revision: number;
  /** Changes whenever the log changes. */
  logs_revision: number;
  models: ModelView[];
  /** Every recognition language, in the order of the Whisper table. */
  languages: Language[];
  accessibility: boolean;
  installation_path?: string | null;
  platform: string;
  downloading: boolean;
  microphone: string;
  can_retry: boolean;
  shortcuts_available: boolean;
  version: string;
  update: UpdateStatus;
};
export type UpdateStatus = { state: string; detail?: string };
export const folderName = (root: string) =>
  root.split(/[\\/]/).filter(Boolean).pop() ?? root;
