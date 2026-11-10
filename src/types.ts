export type Repository = {
  id: string;
  root: string;
  selected: boolean;
  exclusions: string[];
};
export type Mapping = {
  phrase: string;
  replacement: string;
  repository: string | null;
  enabled: boolean;
};
export type Cleanup = {
  mode: string;
  fillers: string[];
  excluded: string[];
  provider: string;
  endpoint: string;
  model: string;
  remote_enabled: boolean;
};
export type Settings = {
  repositories: Repository[];
  mappings: Mapping[];
  cleanup: Cleanup;
  hotkey: string;
  activation: string;
  recognizer: string;
  show_overlay: boolean;
  check_updates: boolean;
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
export type View = {
  settings: Settings;
  phase: string;
  result: ResultText | null;
  error: string;
  indexes: IndexRow[];
  models: [string, boolean][];
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
