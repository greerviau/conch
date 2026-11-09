import { PasteAccess } from "./PasteAccess";
import React, { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Setup, ShortcutRecorder, shortcutLabel } from "./Setup";

type Repository = {
  id: string;
  root: string;
  selected: boolean;
  exclusions: string[];
};
type Mapping = {
  phrase: string;
  replacement: string;
  repository: string | null;
  enabled: boolean;
};
type Cleanup = {
  mode: string;
  fillers: string[];
  excluded: string[];
  provider: string;
  endpoint: string;
  model: string;
  remote_enabled: boolean;
};
type Settings = {
  repositories: Repository[];
  mappings: Mapping[];
  cleanup: Cleanup;
  hotkey: string;
  activation: string;
  show_overlay: boolean;
};
type Candidate = { text: string; repository: string; file: string };
type ResultText = {
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
type View = {
  settings: Settings;
  phase: string;
  result: ResultText | null;
  error: string;
  indexes: [
    string,
    { available: boolean; incomplete: boolean; skipped: number },
    number,
  ][];
  models: [string, boolean][];
  accessibility: boolean;
  installation_path?: string | null;
  platform: string;
  downloading: boolean;
  microphone: string;
  can_retry: boolean;
  shortcuts_available: boolean;
};

export default function App() {
  const [view, setView] = useState<View>();
  const [draft, setDraft] = useState<Settings>();
  const [tab, setTab] = useState("Dictation");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [text, setText] = useState("");
  const [dirty, setDirty] = useState(false);
  const [secret, setSecret] = useState("");
  const [progress, setProgress] = useState("");
  const [root, setRoot] = useState("");
  const [busy, setBusy] = useState(false);
  const [modelBusy, setModelBusy] = useState(false);
  const captureShortcut = useCallback(async (active: boolean) => {
    try {
      await invoke("shortcut_capture", { active });
    } catch (error) {
      setError(String(error));
      throw error;
    }
  }, []);
  const reload = useCallback(async () => {
    try {
      const v = await invoke<View>("view");
      setView(v);
      setDraft((d) => d ?? v.settings);
    } catch (e) {
      setError(String(e));
    }
  }, []);
  useEffect(() => {
    void reload();
    const timer = window.setInterval(() => void reload(), 700);
    return () => window.clearInterval(timer);
  }, [reload]);
  useEffect(() => {
    if (!dirty) setText(view?.result?.text ?? "");
  }, [view?.result?.text, dirty]);
  useEffect(() => {
    const unlisten = listen<[number, number]>("model-progress", (e) =>
      setProgress(`${Math.round((e.payload[0] / e.payload[1]) * 100)}%`),
    );
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);
  async function command(name: string, args?: Record<string, unknown>) {
    setError("");
    setNotice("");
    try {
      await invoke(name, args);
      await reload();
      if (name === "repair_accessibility")
        setNotice(
          "Conch's old Accessibility entry was removed. Enable the installed copy in System Settings, then return here.",
        );
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    }
  }
  async function save(next = draft, contextOnly = false) {
    if (!next) return;
    if (await command("save_settings", { settings: next })) {
      const v = await invoke<View>("view");
      setDraft((current) =>
        contextOnly && current
          ? {
              ...current,
              repositories: current.repositories.map((repository) => ({
                ...repository,
                selected:
                  v.settings.repositories.find((r) => r.id === repository.id)
                    ?.selected ?? repository.selected,
              })),
            }
          : v.settings,
      );
      setView(v);
      setNotice("Settings saved");
    }
  }
  async function downloadModel(model: number) {
    setModelBusy(true);
    setProgress("Starting...");
    try {
      await command("download", { model });
    } finally {
      setModelBusy(false);
      setProgress("");
    }
  }
  async function chooseRepository() {
    try {
      const path = await invoke<string | null>("choose_repository");
      if (path)
        setDraft((current) =>
          current
            ? {
                ...current,
                repositories: [
                  ...current.repositories,
                  {
                    id: crypto.randomUUID(),
                    root: path,
                    selected: true,
                    exclusions: [],
                  },
                ],
              }
            : current,
        );
    } catch (e) {
      setError(String(e));
    }
  }
  async function saveEdit() {
    if (!dirty) return true;
    if (await command("edit", { text })) {
      setDirty(false);
      return true;
    }
    return false;
  }
  if (!view || !draft)
    return (
      <main>
        <h1>Conch</h1>
        <p>{error || "Loading local settings..."}</p>
      </main>
    );
  const needsSetup =
    !view.models[0]?.[1] ||
    (view.platform === "macos" &&
      (!view.accessibility || view.microphone !== "granted"));
  const hasChanges = JSON.stringify(draft) !== JSON.stringify(view.settings);
  const cleanup = draft.cleanup;
  const updateCleanup = (patch: Partial<Cleanup>) =>
    setDraft({ ...draft, cleanup: { ...cleanup, ...patch } });
  const repositoryName = (id: string) =>
    view.settings.repositories
      .find((r) => r.id === id)
      ?.root.split(/[\\/]/)
      .pop() ?? id;
  return (
    <div className="shell">
      <aside>
        <div className="brand">
          <img src="/logo.svg" alt="" />
          <span>Conch</span>
        </div>
        <p className="muted">Local dictation for technical work</p>
        <nav>
          {["Dictation", "Repositories", "Mappings", "Settings"].map((name) => (
            <button
              key={name}
              className={tab === name ? "active" : ""}
              onClick={() => setTab(name)}
            >
              {name}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <span className={`light ${view.phase}`} />
          {view.phase}
          <p>
            {shortcutLabel(view.settings.hotkey, view.platform === "macos")}
          </p>
          <small>
            {view.settings.activation === "push"
              ? "Hold to record"
              : "Press to start or finish"}
            <br />
            180-second recording limit
          </small>
        </div>
      </aside>
      <main>
        <header>
          <div>
            <p className="eyebrow">YOUR WORDS, WITH REPOSITORY CONTEXT</p>
            <h1>{tab}</h1>
          </div>
          <span className="badge">{view.settings.cleanup.mode} cleanup</span>
        </header>
        {(error || view.error) && (
          <div role="alert" className="error">
            {error || view.error}
          </div>
        )}
        {notice && (
          <p role="status" className="notice">
            {notice}
          </p>
        )}
        {tab === "Dictation" && (
          <>
            {needsSetup && (
              <Setup
                mac={view.platform === "macos"}
                installed={!!view.models[0]?.[1]}
                onDownload={() => void downloadModel(0)}
                downloading={modelBusy || view.downloading}
                progress={progress}
                accessibility={view.accessibility || view.platform === "linux"}
                microphone={view.microphone}
                onPermission={() => void command("request_accessibility")}
                onRepair={() => command("repair_accessibility")}
                onRestart={() => void command("restart_for_accessibility")}
                installationPath={view.installation_path}
                busy={view.phase !== "idle" || view.downloading}
                onMicrophone={() => void command("request_microphone")}
                toggle={view.settings.activation === "toggle"}
                clipboardOnly={view.platform === "linux"}
                hotkey={view.settings.hotkey}
                onSettings={() => setTab("Settings")}
              />
            )}
            <section className="record-panel">
              <div>
                <h2>
                  {view.phase === "recording"
                    ? "Listening"
                    : view.phase === "processing"
                      ? "Preparing your result"
                      : !view.models[0]?.[1] ||
                          (view.platform === "macos" &&
                            view.microphone !== "granted")
                        ? "Finish setup to start"
                        : "Ready when you are"}
                </h2>
                <p>
                  Record, review, and keep writing. Conch never submits your
                  prompt.
                </p>
              </div>
              <div className="actions">
                <button
                  className="primary"
                  disabled={
                    view.phase === "processing" ||
                    !view.models[0]?.[1] ||
                    (view.platform === "macos" && view.microphone !== "granted")
                  }
                  onClick={async () => {
                    if (await saveEdit())
                      await command(
                        view.phase === "recording" ? "finish" : "start",
                      );
                  }}
                >
                  {view.phase === "recording"
                    ? "Finish recording"
                    : "Start recording"}
                </button>
                {view.phase !== "idle" && (
                  <button onClick={() => void command("cancel")}>Cancel</button>
                )}
                <button
                  disabled={view.phase !== "idle" || !view.can_retry}
                  onClick={() => void command("retry")}
                >
                  Retry failed recording
                </button>
              </div>
            </section>
            <section>
              <h2>Repository context</h2>
              <p className="muted">
                Selected repositories are captured when recording starts.
              </p>
              <div className="chips">
                {view.settings.repositories.map((r, i) => (
                  <label className="chip" key={r.id}>
                    <input
                      type="checkbox"
                      checked={r.selected}
                      disabled={view.phase !== "idle"}
                      onChange={(e) => {
                        const settings = structuredClone(view.settings);
                        settings.repositories[i].selected = e.target.checked;
                        void save(settings, true);
                      }}
                    />
                    {repositoryName(r.id)}
                  </label>
                ))}
                {view.settings.repositories.length === 0 && (
                  <p>
                    No repositories selected. Dictation and global mappings
                    still work.
                  </p>
                )}
              </div>
            </section>
            <section>
              <div className="section-title">
                <h2>Result</h2>
                <div className="actions">
                  <button
                    disabled={!view.result}
                    onClick={async () => {
                      if (await saveEdit())
                        await command("copy", { raw: false });
                    }}
                  >
                    Copy result
                  </button>
                  <button
                    disabled={!view.result}
                    onClick={() => void command("copy", { raw: true })}
                  >
                    Copy raw transcript
                  </button>
                </div>
              </div>
              <textarea
                aria-label="Editable result"
                className="result"
                disabled={view.phase !== "idle"}
                value={text}
                onChange={(e) => {
                  setText(e.target.value);
                  setDirty(true);
                }}
                onBlur={() => void saveEdit()}
                placeholder="Your completed dictation appears here."
              />
              <p className="muted">
                {view.result?.status ||
                  "The initial result is copied automatically. Eligible focused inputs receive one paste."}
              </p>
              <p className="muted">
                Review edits stay here until you copy them.
              </p>
              {view.result && (
                <details>
                  <summary>Raw transcript and cleanup comparison</summary>
                  <h3>Raw transcript</h3>
                  <pre>{view.result.raw}</pre>
                  <h3>Basic result</h3>
                  <pre>{view.result.basic}</pre>
                  <h3>Current result</h3>
                  <pre>{view.result.text}</pre>
                </details>
              )}
            </section>
            {!!view.result?.edits.length && (
              <section>
                <h2>Accepted edits</h2>
                {view.result.edits.map((e, i) => (
                  <div className="candidate" key={i}>
                    <span>
                      <code>{e.original}</code> to <code>{e.replacement}</code>
                    </span>
                    <button
                      disabled={dirty || view.phase !== "idle"}
                      onClick={() => void command("undo", { index: i })}
                    >
                      Undo
                    </button>
                  </div>
                ))}
              </section>
            )}
            {!!view.result?.suggestions.length && (
              <section>
                <h2>Review suggestions</h2>
                {view.result.suggestions.map((s, i) => (
                  <article className="suggestion" key={`${i}-${s.original}`}>
                    <p>
                      {s.conflict
                        ? "Mapping conflict"
                        : "Repository suggestion"}
                      : <code>{s.original}</code>
                    </p>
                    {s.candidates.map((c, j) => (
                      <div className="candidate" key={`${c.repository}-${j}`}>
                        <div>
                          <code>{c.text}</code>
                          <small>
                            {repositoryName(c.repository)} / {c.file}
                          </small>
                        </div>
                        <button
                          disabled={dirty || view.phase !== "idle"}
                          onClick={() =>
                            void command("accept", { index: i, candidate: j })
                          }
                        >
                          Accept
                        </button>
                      </div>
                    ))}
                    <button
                      disabled={dirty || view.phase !== "idle"}
                      onClick={() => void command("reject", { index: i })}
                    >
                      Keep original
                    </button>
                  </article>
                ))}
              </section>
            )}
          </>
        )}
        {tab === "Repositories" && (
          <section>
            <h2>Configured repositories</h2>
            <p>
              Conch reads filenames and Python, JavaScript, and TypeScript
              declarations. Ignored, binary, generated, oversized, and external
              symlink files are excluded.
            </p>
            {draft.repositories.map((r, i) => {
              const status = view.indexes.find((x) => x[0] === r.id);
              return (
                <article className="repo" key={r.id || i}>
                  <strong>{r.root}</strong>
                  <p className="muted">
                    {status
                      ? `${status[1].available ? "Available" : "Unavailable"} / ${status[1].incomplete ? "Incomplete index" : "Index completed"} / ${status[1].skipped} symbol extractions skipped / ${new Date(status[2] * 1000).toLocaleString()}`
                      : "Refresh needed"}
                  </p>
                  <label>
                    Extra exclusions (one ignore pattern per line)
                    <textarea
                      value={r.exclusions.join("\n")}
                      onChange={(e) =>
                        setDraft({
                          ...draft,
                          repositories: draft.repositories.map((v, n) =>
                            n === i
                              ? {
                                  ...v,
                                  exclusions: e.target.value
                                    .split("\n")
                                    .filter(Boolean),
                                }
                              : v,
                          ),
                        })
                      }
                    />
                  </label>
                  <button
                    onClick={() =>
                      setDraft({
                        ...draft,
                        repositories: draft.repositories.filter(
                          (_, n) => n !== i,
                        ),
                      })
                    }
                  >
                    Remove repository
                  </button>
                </article>
              );
            })}
            <button className="primary" onClick={() => void chooseRepository()}>
              Choose repository folder
            </button>
            <details>
              <summary>Enter a path manually</summary>
              <label>
                Absolute repository path
                <input
                  value={root}
                  onChange={(e) => setRoot(e.target.value)}
                  placeholder="/Users/me/work/project"
                />
              </label>
            </details>
            <div className="actions">
              <button
                onClick={() => {
                  if (root.trim()) {
                    setDraft({
                      ...draft,
                      repositories: [
                        ...draft.repositories,
                        {
                          id: crypto.randomUUID(),
                          root: root.trim(),
                          selected: true,
                          exclusions: [],
                        },
                      ],
                    });
                    setRoot("");
                  }
                }}
              >
                Add repository
              </button>
              <button className="primary" onClick={() => void save()}>
                Save repositories
              </button>
              <button
                disabled={busy}
                onClick={async () => {
                  setBusy(true);
                  await command("refresh");
                  setBusy(false);
                }}
              >
                Refresh indexes
              </button>
            </div>
          </section>
        )}
        {tab === "Mappings" && (
          <section>
            <h2>Exact phrase replacements</h2>
            <p>
              Longest whole phrases apply once. Repository mappings override
              global mappings. Conflicts wait for your choice.
            </p>
            {draft.mappings.map((m, i) => {
              const update = (patch: Partial<Mapping>) =>
                setDraft({
                  ...draft,
                  mappings: draft.mappings.map((v, n) =>
                    n === i ? { ...v, ...patch } : v,
                  ),
                });
              return (
                <article className="mapping" key={i}>
                  <label className="inline">
                    <input
                      type="checkbox"
                      checked={m.enabled}
                      onChange={(e) => update({ enabled: e.target.checked })}
                    />
                    Enabled
                  </label>
                  <label>
                    Spoken phrase
                    <input
                      value={m.phrase}
                      onChange={(e) => update({ phrase: e.target.value })}
                    />
                  </label>
                  <label>
                    Exact replacement
                    <input
                      value={m.replacement}
                      onChange={(e) => update({ replacement: e.target.value })}
                    />
                  </label>
                  <label>
                    Scope
                    <select
                      value={m.repository ?? ""}
                      onChange={(e) =>
                        update({ repository: e.target.value || null })
                      }
                    >
                      <option value="">Global</option>
                      {draft.repositories.map((r) => (
                        <option key={r.id} value={r.id}>
                          {r.root}
                        </option>
                      ))}
                    </select>
                  </label>
                  <button
                    onClick={() =>
                      setDraft({
                        ...draft,
                        mappings: draft.mappings.filter((_, n) => n !== i),
                      })
                    }
                  >
                    Remove
                  </button>
                </article>
              );
            })}
            <div className="actions">
              <button
                onClick={() =>
                  setDraft({
                    ...draft,
                    mappings: [
                      ...draft.mappings,
                      {
                        phrase: "",
                        replacement: "",
                        repository: null,
                        enabled: true,
                      },
                    ],
                  })
                }
              >
                Add mapping
              </button>
              <button className="primary" onClick={() => void save()}>
                Save mappings
              </button>
            </div>
          </section>
        )}
        {tab === "Settings" && (
          <>
            <section>
              <h2>Recording and delivery</h2>
              <label>Global shortcut</label>
              <ShortcutRecorder
                value={draft.hotkey}
                mac={view.platform === "macos"}
                onCapture={captureShortcut}
                onChange={(hotkey) => setDraft({ ...draft, hotkey })}
              />
              {!view.shortcuts_available && (
                <p className="error">
                  Global shortcuts are unavailable on this desktop. Use the
                  recording controls.
                </p>
              )}
              <label>
                Activation
                <select
                  value={draft.activation}
                  onChange={(e) =>
                    setDraft({ ...draft, activation: e.target.value })
                  }
                >
                  <option value="push">Push-to-talk</option>
                  <option value="toggle">Toggle recording</option>
                </select>
              </label>
              <label className="inline">
                <input
                  type="checkbox"
                  checked={draft.show_overlay}
                  onChange={(e) =>
                    setDraft({ ...draft, show_overlay: e.target.checked })
                  }
                />
                Show a floating recording indicator over other apps
              </label>
              <div className="permission-grid">
                <article>
                  <h3>Microphone</h3>
                  <p>
                    {view.microphone === "system"
                      ? "Your system manages microphone access. Check its privacy settings if recording fails."
                      : view.microphone === "granted"
                        ? "Ready to record"
                        : view.microphone === "denied"
                          ? "Permission denied"
                          : "Not checked yet"}
                  </p>
                  {view.platform === "macos" &&
                    view.microphone !== "granted" && (
                      <button
                        onClick={() => void command("request_microphone")}
                      >
                        Check microphone access
                      </button>
                    )}
                </article>
                <article>
                  <h3>Automatic paste</h3>
                  {view.platform === "macos" ? (
                    <PasteAccess
                      granted={view.accessibility}
                      path={view.installation_path}
                      busy={view.phase !== "idle" || view.downloading}
                      onOpen={() => void command("request_accessibility")}
                      onRepair={() => command("repair_accessibility")}
                      onRestart={() =>
                        void command("restart_for_accessibility")
                      }
                    />
                  ) : (
                    <p>
                      {view.platform === "linux"
                        ? "Conch copies the result; paste manually into the receiving app."
                        : "Windows UI Automation checks eligible editable controls."}
                    </p>
                  )}
                </article>
              </div>
              {view.platform === "macos" && (
                <p className="muted">
                  After enabling Conch, return here to check the status. Local
                  app updates can require removing and re-adding Conch in
                  Accessibility settings.
                </p>
              )}
              <p>
                Windows uses UI Automation for eligible Edit controls. Linux
                exposes clipboard fallback. Windows, X11, and Wayland behavior
                requires platform validation.
              </p>
            </section>
            <section>
              <h2>Transcript cleanup</h2>
              <label>
                Mode
                <select
                  value={cleanup.mode}
                  onChange={(e) => updateCleanup({ mode: e.target.value })}
                >
                  <option value="off">Off</option>
                  <option value="basic">Basic</option>
                  <option value="enhanced">Enhanced</option>
                </select>
              </label>
              {cleanup.mode === "enhanced" &&
                cleanup.provider === "managed" &&
                !view.models[1]?.[1] && (
                  <div className="remote">
                    <p>
                      Enhanced cleanup needs the optional S1-mini model (484
                      MB). Basic cleanup works immediately.
                    </p>
                    <button
                      disabled={modelBusy || view.downloading}
                      onClick={() => void downloadModel(1)}
                    >
                      Download optional cleanup model
                    </button>
                  </div>
                )}
              <label>
                Hesitation tokens (comma separated)
                <input
                  value={cleanup.fillers.join(",")}
                  onChange={(e) =>
                    updateCleanup({ fillers: e.target.value.split(",") })
                  }
                />
              </label>
              <label>
                Excluded literal terms (comma separated)
                <input
                  value={cleanup.excluded.join(",")}
                  onChange={(e) =>
                    updateCleanup({ excluded: e.target.value.split(",") })
                  }
                />
              </label>
              <label>
                Enhanced provider
                <select
                  value={cleanup.provider}
                  onChange={(e) =>
                    updateCleanup({
                      provider: e.target.value,
                      remote_enabled: false,
                    })
                  }
                >
                  <option value="managed">Managed S1-mini</option>
                  <option value="local">Local endpoint</option>
                  <option value="remote">Remote endpoint</option>
                </select>
              </label>
              {cleanup.provider !== "managed" && (
                <>
                  <label>
                    Full chat-completions endpoint
                    <input
                      value={cleanup.endpoint}
                      onChange={(e) =>
                        updateCleanup({
                          endpoint: e.target.value,
                          remote_enabled: false,
                        })
                      }
                      placeholder="http://127.0.0.1:8080/v1/chat/completions"
                    />
                  </label>
                  <label>
                    Model
                    <input
                      value={cleanup.model}
                      onChange={(e) => updateCleanup({ model: e.target.value })}
                    />
                  </label>
                </>
              )}
              {cleanup.provider === "remote" && (
                <div className="remote">
                  <p>
                    Destination: {cleanup.endpoint || "Set an HTTPS endpoint"}
                  </p>
                  <p>
                    Dictated text leaves this device. The provider may charge
                    for requests. Protected spans use placeholders, but the
                    transcript can still contain confidential text. Conch sends
                    no audio, source files, vocabulary indexes, or clipboard
                    content.
                  </p>
                  <label className="inline">
                    <input
                      type="checkbox"
                      checked={cleanup.remote_enabled}
                      onChange={(e) =>
                        updateCleanup({ remote_enabled: e.target.checked })
                      }
                    />
                    Enable remote cleanup for this destination
                  </label>
                  <label>
                    API key
                    <input
                      type="password"
                      autoComplete="off"
                      value={secret}
                      onChange={(e) => setSecret(e.target.value)}
                    />
                  </label>
                  <div className="actions">
                    <button
                      onClick={async () => {
                        if (
                          await command("credential", {
                            endpoint: cleanup.endpoint,
                            secret,
                          })
                        ) {
                          setSecret("");
                          setNotice("Credential saved in the platform store");
                        }
                      }}
                    >
                      Save key
                    </button>
                    <button
                      onClick={async () => {
                        if (
                          await command("credential", {
                            endpoint: cleanup.endpoint,
                            secret: null,
                          })
                        ) {
                          setSecret("");
                          setNotice("Credential removed");
                        }
                      }}
                    >
                      Remove key
                    </button>
                  </div>
                </div>
              )}
              <p className="muted">
                Enhanced failures use Basic. Preservation checks do not prove
                semantic equivalence. Enhanced input is limited to 3,000 bytes.
              </p>
              <button className="primary" onClick={() => void save()}>
                Save settings
              </button>
            </section>
            <section>
              <h2>Local models</h2>
              <p>
                Explicit downloads are required once. Core dictation then works
                offline.
              </p>
              {view.models.map(([name, installed], i) => (
                <article className="model" key={name}>
                  <div>
                    <strong>
                      {i === 0 ? "Whisper tiny.en" : "S1-mini by Superwhisper"}
                    </strong>
                    <p>
                      {i === 0
                        ? "English recognition / 77.7 MB"
                        : "Optional English cleanup / 484.2 MB"}
                    </p>
                    <small>{installed ? "Installed" : "Not installed"}</small>
                  </div>
                  <button
                    disabled={modelBusy || view.downloading || installed}
                    onClick={async () => {
                      await downloadModel(i);
                    }}
                  >
                    Download
                  </button>
                </article>
              ))}
              <p role="status">
                {view.downloading ? `Downloading ${progress}` : progress}
              </p>
              <details>
                <summary>Licenses and acknowledgments</summary>
                <p>
                  whisper.cpp and llama.cpp use the MIT license. S1-mini by
                  Superwhisper uses Apache 2.0 with an additional naming
                  condition. Model LICENSE and NOTICE files ship with Conch.
                </p>
              </details>
            </section>
          </>
        )}
        {hasChanges && (
          <footer className="save-bar">
            <span>Unsaved changes</span>
            <button onClick={() => setDraft(structuredClone(view.settings))}>
              Discard changes
            </button>
            <button
              className="primary"
              disabled={view.phase !== "idle"}
              onClick={() => void save()}
            >
              Save changes
            </button>
          </footer>
        )}
      </main>
    </div>
  );
}
