import { PasteAccess } from "./PasteAccess";
import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Setup, ShortcutRecorder, shortcutLabel } from "./Setup";
import { Mappings } from "./Mappings";
import { Repositories } from "./Repositories";
import { Mic, Stop } from "./icons";
import { Toggle } from "./ui";
import { folderName } from "./types";
import type { Cleanup, Settings, View } from "./types";

const TABS = ["Dictation", "Repositories", "Mappings", "Settings"];

function Row({
  title,
  note,
  children,
}: {
  title: string;
  note?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="row">
      <div className="row-text">
        <b>{title}</b>
        {note && <span>{note}</span>}
      </div>
      {children && <div className="row-control">{children}</div>}
    </div>
  );
}

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
      <main className="page">
        <h1>Conch</h1>
        <p>{error || "Loading local settings..."}</p>
      </main>
    );
  const modelInstalled = !!view.models[0]?.[1];
  const microphoneReady =
    view.platform !== "macos" || view.microphone === "granted";
  const needsSetup =
    !modelInstalled ||
    (view.platform === "macos" &&
      (!view.accessibility || view.microphone !== "granted"));
  const canRecord = modelInstalled && microphoneReady;
  const showWorkspace = canRecord || !!view.result;
  const hasChanges = JSON.stringify(draft) !== JSON.stringify(view.settings);
  const cleanup = draft.cleanup;
  const updateCleanup = (patch: Partial<Cleanup>) =>
    setDraft({ ...draft, cleanup: { ...cleanup, ...patch } });
  const repositoryName = (id: string) => {
    const found = view.settings.repositories.find((r) => r.id === id);
    return found ? folderName(found.root) : id;
  };
  const recording = view.phase === "recording";
  const processing = view.phase === "processing";
  const hotkey = (
    <kbd>{shortcutLabel(view.settings.hotkey, view.platform === "macos")}</kbd>
  );
  const toggleMode = view.settings.activation === "toggle";
  const heroTitle = recording
    ? "Listening"
    : processing
      ? "Preparing your result"
      : canRecord
        ? "Ready when you are"
        : "Finish setup to start";
  const heroHint = recording ? (
    toggleMode ? (
      <>Press the shortcut again, or click to stop.</>
    ) : (
      <>Release the shortcut to finish, or click to stop.</>
    )
  ) : processing ? (
    <>Applying mappings and cleanup.</>
  ) : !canRecord ? null : !view.shortcuts_available ? (
    <>Click the button to record.</>
  ) : toggleMode ? (
    <>
      Press {hotkey} in any app to start and again to finish, or click the
      button.
    </>
  ) : (
    <>Hold {hotkey} in any app, or click the button.</>
  );
  const statusLabel = recording
    ? "Recording"
    : processing
      ? "Working"
      : needsSetup
        ? "Setup needed"
        : "Ready";
  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <img src="/icon.svg" alt="" />
          <span>Conch</span>
        </div>
        <nav aria-label="Pages">
          {TABS.map((name) => (
            <button
              key={name}
              className={tab === name ? "active" : ""}
              aria-current={tab === name ? "page" : undefined}
              onClick={() => setTab(name)}
            >
              {name}
            </button>
          ))}
        </nav>
        <div className="state" role="status" aria-label="Conch status">
          <span
            className={`light ${view.phase}${needsSetup ? " setup" : ""}`}
          />
          {statusLabel}
        </div>
      </header>
      <main className="page">
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
                installed={modelInstalled}
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
                toggle={toggleMode}
                clipboardOnly={view.platform === "linux"}
                hotkey={view.settings.hotkey}
                onSettings={() => setTab("Settings")}
              />
            )}
            <section className={`hero${canRecord ? "" : " idle"}`}>
              <button
                className={`record${recording ? " on" : ""}`}
                aria-label={recording ? "Finish recording" : "Start recording"}
                disabled={processing || !canRecord}
                onClick={async () => {
                  if (await saveEdit())
                    await command(recording ? "finish" : "start");
                }}
              >
                {recording ? <Stop /> : <Mic />}
              </button>
              <h2>{heroTitle}</h2>
              {heroHint && <p>{heroHint}</p>}
              {canRecord && view.phase === "idle" && (
                <p className="note">Recordings stop after 180 seconds.</p>
              )}
              {(view.phase !== "idle" || view.can_retry) && (
                <div className="actions">
                  {view.phase !== "idle" && (
                    <button
                      className="ghost"
                      onClick={() => void command("cancel")}
                    >
                      Cancel
                    </button>
                  )}
                  {view.phase === "idle" && view.can_retry && (
                    <button
                      className="ghost"
                      onClick={() => void command("retry")}
                    >
                      Retry failed recording
                    </button>
                  )}
                </div>
              )}
            </section>
            {showWorkspace && (
              <>
                <div className="chips" aria-label="Repository context">
                  <small>Context</small>
                  {view.settings.repositories.map((r, i) => (
                    <label
                      className={`chip${r.selected ? " on" : ""}`}
                      key={r.id}
                      title="Selected repositories are captured when recording starts."
                    >
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
                      {folderName(r.root)}
                    </label>
                  ))}
                  {view.settings.repositories.length === 0 && (
                    <small>
                      No repositories selected. Dictation and global mappings
                      still work.
                    </small>
                  )}
                </div>
                <section className="card">
                  <div className="card-head">
                    <h2>Result</h2>
                    <div className="actions">
                      <button
                        className="ghost"
                        disabled={!view.result}
                        onClick={() => void command("copy", { raw: true })}
                      >
                        Copy raw transcript
                      </button>
                      <button
                        disabled={!view.result}
                        onClick={async () => {
                          if (await saveEdit())
                            await command("copy", { raw: false });
                        }}
                      >
                        Copy result
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
                  <p className="note">
                    {view.result?.status ||
                      "The initial result is copied automatically. Eligible focused inputs receive one paste."}{" "}
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
              </>
            )}
            {!!view.result?.edits.length && (
              <section className="card">
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
              <section className="card">
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
                        {!s.conflict && (
                          <button
                            disabled={dirty || view.phase !== "idle"}
                            onClick={() =>
                              void command("accept_always", {
                                index: i,
                                candidate: j,
                              })
                            }
                          >
                            Accept and always replace
                          </button>
                        )}
                      </div>
                    ))}
                    <button
                      className="ghost"
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
          <Repositories
            repositories={draft.repositories}
            indexes={view.indexes}
            refreshing={busy}
            onChange={(repositories) => setDraft({ ...draft, repositories })}
            onChoose={() => void chooseRepository()}
            onRefresh={async () => {
              setBusy(true);
              await command("refresh");
              setBusy(false);
            }}
          />
        )}
        {tab === "Mappings" && (
          <Mappings
            mappings={draft.mappings}
            repositories={draft.repositories}
            onChange={(mappings) => setDraft({ ...draft, mappings })}
          />
        )}
        {tab === "Settings" && (
          <>
            <h3 className="group">Recording</h3>
            <div className="card rows">
              <Row
                title="Shortcut"
                note="Works in any app. Click Change shortcut, then press your key combination."
              >
                <ShortcutRecorder
                  value={draft.hotkey}
                  mac={view.platform === "macos"}
                  onCapture={captureShortcut}
                  onChange={async (hotkey) => {
                    if (await command("set_hotkey", { hotkey }))
                      setDraft((d) => d && { ...d, hotkey });
                  }}
                />
              </Row>
              {!view.shortcuts_available && (
                <p className="error">
                  Global shortcuts are unavailable on this desktop. Use the
                  recording controls.
                </p>
              )}
              <Row title="Activation">
                <select
                  aria-label="Activation"
                  value={draft.activation}
                  onChange={(e) =>
                    setDraft({ ...draft, activation: e.target.value })
                  }
                >
                  <option value="push">Push-to-talk</option>
                  <option value="toggle">Toggle recording</option>
                </select>
              </Row>
              <Row
                title="Floating indicator"
                note="Show a floating recording indicator over other apps."
              >
                <Toggle
                  label="Floating indicator"
                  checked={draft.show_overlay}
                  onChange={(show_overlay) =>
                    setDraft({ ...draft, show_overlay })
                  }
                />
              </Row>
            </div>
            <h3 className="group">Access</h3>
            <div className="card rows">
              <Row
                title="Microphone"
                note={
                  view.microphone === "system"
                    ? "Your system manages microphone access. Check its privacy settings if recording fails."
                    : view.microphone === "granted"
                      ? "Ready to record"
                      : view.microphone === "denied"
                        ? "Permission denied"
                        : "Not checked yet"
                }
              >
                {view.platform === "macos" && view.microphone !== "granted" && (
                  <button onClick={() => void command("request_microphone")}>
                    Check microphone access
                  </button>
                )}
              </Row>
              <div className="row stack">
                <div className="row-text">
                  <b>Automatic paste</b>
                </div>
                {view.platform === "macos" ? (
                  <PasteAccess
                    granted={view.accessibility}
                    path={view.installation_path}
                    busy={view.phase !== "idle" || view.downloading}
                    onOpen={() => void command("request_accessibility")}
                    onRepair={() => command("repair_accessibility")}
                    onRestart={() => void command("restart_for_accessibility")}
                  />
                ) : (
                  <p>
                    {view.platform === "linux"
                      ? "Conch copies the result; paste manually into the receiving app."
                      : "Windows UI Automation checks eligible editable controls."}
                  </p>
                )}
                <p className="note">
                  {view.platform === "macos"
                    ? "After enabling Conch, return here to check the status. Local app updates can require removing and re-adding Conch in Accessibility settings."
                    : "Windows, X11, and Wayland behavior requires platform validation."}
                </p>
              </div>
            </div>
            <h3 className="group">Transcript cleanup</h3>
            <div className="card rows">
              <Row title="Mode">
                <select
                  aria-label="Mode"
                  value={cleanup.mode}
                  onChange={(e) => updateCleanup({ mode: e.target.value })}
                >
                  <option value="off">Off</option>
                  <option value="basic">Basic</option>
                  <option value="enhanced">Enhanced</option>
                </select>
              </Row>
              {cleanup.mode === "enhanced" &&
                cleanup.provider === "managed" &&
                !view.models[1]?.[1] && (
                  <div className="callout">
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
              <Row title="Hesitation tokens" note="Comma separated">
                <input
                  aria-label="Hesitation tokens"
                  value={cleanup.fillers.join(",")}
                  onChange={(e) =>
                    updateCleanup({ fillers: e.target.value.split(",") })
                  }
                />
              </Row>
              <Row title="Excluded literal terms" note="Comma separated">
                <input
                  aria-label="Excluded literal terms"
                  value={cleanup.excluded.join(",")}
                  onChange={(e) =>
                    updateCleanup({ excluded: e.target.value.split(",") })
                  }
                />
              </Row>
              <Row title="Enhanced provider">
                <select
                  aria-label="Enhanced provider"
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
              </Row>
              {cleanup.provider !== "managed" && (
                <>
                  <Row title="Endpoint" note="Full chat-completions endpoint">
                    <input
                      aria-label="Endpoint"
                      value={cleanup.endpoint}
                      onChange={(e) =>
                        updateCleanup({
                          endpoint: e.target.value,
                          remote_enabled: false,
                        })
                      }
                      placeholder="http://127.0.0.1:8080/v1/chat/completions"
                    />
                  </Row>
                  <Row title="Model">
                    <input
                      aria-label="Model"
                      value={cleanup.model}
                      onChange={(e) => updateCleanup({ model: e.target.value })}
                    />
                  </Row>
                </>
              )}
              {cleanup.provider === "remote" && (
                <div className="callout">
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
                  <label className="check">
                    <input
                      type="checkbox"
                      checked={cleanup.remote_enabled}
                      onChange={(e) =>
                        updateCleanup({ remote_enabled: e.target.checked })
                      }
                    />
                    Enable remote cleanup for this destination
                  </label>
                  <label className="field">
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
                      className="ghost"
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
              <p className="note">
                Enhanced failures use Basic. Preservation checks do not prove
                semantic equivalence. Enhanced input is limited to 3,000 bytes.
              </p>
            </div>
            <h3 className="group">Local models</h3>
            <div className="card rows">
              {view.models.map(([name, installed], i) => (
                <Row
                  key={name}
                  title={
                    i === 0 ? "Whisper tiny.en" : "S1-mini by Superwhisper"
                  }
                  note={
                    i === 0
                      ? "English recognition / 77.7 MB"
                      : "Optional English cleanup / 484.2 MB"
                  }
                >
                  {installed ? (
                    <span className="installed">Installed</span>
                  ) : (
                    <button
                      disabled={modelBusy || view.downloading}
                      onClick={() => void downloadModel(i)}
                    >
                      Download
                    </button>
                  )}
                </Row>
              ))}
              <p className="note">
                Explicit downloads are required once. Core dictation then works
                offline.
              </p>
              {(view.downloading || progress) && (
                <p role="status">
                  {view.downloading ? `Downloading ${progress}` : progress}
                </p>
              )}
              <details>
                <summary>Licenses and acknowledgments</summary>
                <p>
                  whisper.cpp and llama.cpp use the MIT license. S1-mini by
                  Superwhisper uses Apache 2.0 with an additional naming
                  condition. Model LICENSE and NOTICE files ship with Conch.
                </p>
              </details>
            </div>
          </>
        )}
        {hasChanges && (
          <footer className="savebar">
            <span>Unsaved changes</span>
            <button
              className="ghost"
              onClick={() => setDraft(structuredClone(view.settings))}
            >
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
