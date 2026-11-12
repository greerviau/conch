import { useState } from "react";
import {
  LanguageMenu,
  ModelStatus,
  RECOGNIZER_TRADEOFF,
  RecognizerMenu,
  voiceDetectionNote,
} from "./ModelChoice";
import {
  cleanupModel,
  modelForLanguage,
  recognizerInfo,
  size,
  supports,
  voiceDetection,
} from "./models";
import { Row, Toggle } from "./ui";
import {
  activeProfile,
  folderName,
  type Cleanup,
  type IndexRow,
  type Language,
  type ModelView,
  type Profile,
  type Settings,
} from "./types";

export const ENHANCED_LABELS: Record<string, string> = {
  disabled: "Basic only",
  managed: "Managed S1-mini",
  local: "Local endpoint",
  remote: "Remote endpoint",
};

const plural = (n: number, one: string, many: string) =>
  `${n} ${n === 1 ? one : many}`;

/** "Small (English), Basic only, 1 repository" for the profile list. */
export function profileSummary(profile: Profile, models: ModelView[]) {
  return [
    recognizerInfo(models, profile.recognizer).label,
    ENHANCED_LABELS[profile.enhanced] ?? profile.enhanced,
    profile.repositories.length
      ? plural(profile.repositories.length, "repository", "repositories")
      : "No repositories",
  ].join(", ");
}

export function Profiles({
  settings,
  onChange,
  models,
  languages,
  indexes,
  ready,
  downloadingId,
  downloadBusy,
  percent,
  onDownload,
  onCredential,
}: {
  /** The draft settings; changes wait for Save changes. */
  settings: Settings;
  onChange: (settings: Settings) => void;
  models: ModelView[];
  languages: Language[];
  indexes: IndexRow[];
  /** Whether a model and everything it needs are on disk. */
  ready: (id: string) => boolean;
  downloadingId: string | null;
  downloadBusy: boolean;
  percent: number | null;
  onDownload: (id: string) => void;
  /** Saves or, with null, removes the API key for an endpoint. */
  onCredential: (endpoint: string, secret: string | null) => Promise<boolean>;
}) {
  const [editingId, setEditingId] = useState(settings.active_profile);
  const [secret, setSecret] = useState("");
  const profile =
    settings.profiles.find((p) => p.id === editingId) ??
    activeProfile(settings);
  const active = activeProfile(settings).id;
  const update = (patch: Partial<Profile>) =>
    onChange({
      ...settings,
      profiles: settings.profiles.map((p) =>
        p.id === profile.id ? { ...p, ...patch } : p,
      ),
    });
  const cleanup = settings.cleanup;
  const updateCleanup = (patch: Partial<Cleanup>) =>
    onChange({ ...settings, cleanup: { ...cleanup, ...patch } });
  const add = (from: Profile, name: string) => {
    const added: Profile = { ...from, id: crypto.randomUUID(), name };
    onChange({ ...settings, profiles: [...settings.profiles, added] });
    setEditingId(added.id);
  };
  const remove = () => {
    const profiles = settings.profiles.filter((p) => p.id !== profile.id);
    onChange({
      ...settings,
      profiles,
      active_profile:
        settings.active_profile === profile.id
          ? profiles[0].id
          : settings.active_profile,
    });
    setEditingId(profile.id === active ? profiles[0].id : active);
  };
  const recognizer = recognizerInfo(models, profile.recognizer);
  const vad = voiceDetection(models);
  const voiceDetectionOnly =
    models.some((m) => m.id === profile.recognizer && m.ready) &&
    !ready(vad.id);
  const mini = cleanupModel(models);
  const managedUnsupported =
    profile.enhanced === "managed" && !supports(mini, profile.language);
  const languageName =
    profile.language === "auto"
      ? "Auto-detect"
      : (languages.find((l) => l.code === profile.language)?.name ??
        profile.language);
  return (
    <>
      <h1>Profiles</h1>
      <p className="intro">
        A profile sets the recognition model, cleanup, and repository context of
        a recording. Switch profiles on the Dictation page, from the menu bar
        icon, or on the recording indicator.
      </p>
      <div className="split">
        <div className="card plist" role="list" aria-label="Profiles">
          {settings.profiles.map((p) => (
            <button
              key={p.id}
              role="listitem"
              className={p.id === profile.id ? "active" : ""}
              aria-current={p.id === profile.id ? "true" : undefined}
              onClick={() => setEditingId(p.id)}
            >
              <span>
                <span className="pname">{p.name || "Unnamed profile"}</span>
                <span className="pmeta">{profileSummary(p, models)}</span>
              </span>
              {p.id === active && <span className="badge">Active</span>}
            </button>
          ))}
          <button
            className="add"
            onClick={() =>
              add(
                {
                  ...profile,
                  enhanced: "disabled",
                  repositories: [],
                  auto_accept: false,
                },
                "New profile",
              )
            }
          >
            + New profile
          </button>
        </div>
        <div>
          <h3 className="group">General</h3>
          <div className="card rows">
            <Row title="Name">
              <input
                aria-label="Profile name"
                value={profile.name}
                onChange={(e) => update({ name: e.target.value })}
              />
            </Row>
          </div>
          {!profile.name.trim() && (
            <p className="error">Give the profile a name before saving.</p>
          )}
          <h3 className="group">Recognition</h3>
          <div className="card rows">
            <Row
              title="Language"
              note={`The language you speak. Models that cannot recognize it are hidden.${
                profile.language === "auto"
                  ? " Auto-detect is less reliable for recordings under a few seconds. The first chunk of a recording with speech sets the language for the rest of it."
                  : ""
              }${
                profile.language === "en"
                  ? ""
                  : ' Spelled-letter matching of acronyms, such as "pod gee see", works for English only.'
              }`}
            >
              <LanguageMenu
                languages={languages}
                models={models}
                value={profile.language}
                disabled={downloadBusy}
                onChange={(code) =>
                  update({
                    language: code,
                    recognizer: modelForLanguage(
                      models,
                      code,
                      profile.recognizer,
                    ),
                  })
                }
              />
            </Row>
            <Row title="Recognition model" note={RECOGNIZER_TRADEOFF}>
              <RecognizerMenu
                models={models}
                language={profile.language}
                value={profile.recognizer}
                onChange={(id) => update({ recognizer: id })}
                ready={ready}
                disabled={downloadBusy}
              />
            </Row>
            <ModelStatus
              ready={ready(profile.recognizer)}
              downloading={downloadingId === profile.recognizer}
              percent={percent}
              blocked={downloadBusy}
              label={recognizer.label}
              readyText={`${recognizer.label} is installed. Speech stays on this device.`}
              needText={
                voiceDetectionOnly
                  ? "This model needs the voice detection model before you can record with it."
                  : "This model needs a one-time download before you can record with it."
              }
              button={
                voiceDetectionOnly
                  ? `Download ${vad.label}, ${size(vad)}`
                  : `Download ${recognizer.label}, ${size(recognizer)}`
              }
              note={voiceDetectionOnly ? undefined : voiceDetectionNote(models)}
              onDownload={() => onDownload(profile.recognizer)}
            />
          </div>
          <h3 className="group">Transcript cleanup</h3>
          <div className="card rows">
            <Row
              title="Enhanced cleanup"
              note={`Basic cleanup always runs. Enhanced cleanup rewrites the transcript with a language model after it.${
                managedUnsupported
                  ? ` Managed S1-mini supports English only; Conch uses Basic cleanup for ${languageName}. A local or remote endpoint works in any language.`
                  : ""
              }`}
            >
              <select
                aria-label="Enhanced cleanup"
                value={profile.enhanced}
                onChange={(e) => update({ enhanced: e.target.value })}
              >
                <option value="disabled">Disabled</option>
                <option value="managed">Managed S1-mini</option>
                <option value="local">Local endpoint</option>
                <option value="remote">Remote endpoint</option>
              </select>
            </Row>
            {profile.enhanced === "managed" && (
              <ModelStatus
                ready={ready(mini.id)}
                downloading={downloadingId === mini.id}
                percent={percent}
                blocked={downloadBusy}
                label={mini.label}
                readyText={`${mini.label} is installed.`}
                needText="Managed cleanup needs the optional S1-mini model. Until it is installed, Conch uses Basic cleanup."
                button={`Download ${mini.label}, ${size(mini)}`}
                onDownload={() => onDownload(mini.id)}
              />
            )}
            {(profile.enhanced === "local" ||
              profile.enhanced === "remote") && (
              <>
                <Row
                  title="Endpoint"
                  note="Full chat-completions endpoint. Every profile that uses an endpoint shares it."
                >
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
            {profile.enhanced === "remote" && (
              <div className="callout">
                <p>
                  Destination: {cleanup.endpoint || "Set an HTTPS endpoint"}
                </p>
                <p>
                  Dictated text leaves this device. The provider may charge for
                  requests. Protected spans use placeholders, but the transcript
                  can still contain confidential text. Conch sends no audio,
                  source files, vocabulary indexes, or clipboard content.
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
                      if (await onCredential(cleanup.endpoint, secret))
                        setSecret("");
                    }}
                  >
                    Save key
                  </button>
                  <button
                    className="ghost"
                    onClick={async () => {
                      if (await onCredential(cleanup.endpoint, null))
                        setSecret("");
                    }}
                  >
                    Remove key
                  </button>
                </div>
              </div>
            )}
            {profile.enhanced !== "disabled" && (
              <p className="note">
                Enhanced failures use Basic. Preservation checks do not prove
                semantic equivalence. Enhanced input is limited to 3,000 bytes.
              </p>
            )}
          </div>
          <h3 className="group">Repository context</h3>
          <div className="card">
            {settings.repositories.length === 0 ? (
              <p className="note" style={{ marginTop: 0 }}>
                Add repositories on the Repositories page to use their
                identifiers here.
              </p>
            ) : (
              <div className="repolist">
                {settings.repositories.map((r) => {
                  const terms = indexes.find((x) => x[0] === r.id)?.[1].entries;
                  return (
                    <label key={r.id}>
                      <input
                        type="checkbox"
                        checked={profile.repositories.includes(r.id)}
                        onChange={(e) =>
                          update({
                            repositories: e.target.checked
                              ? [...profile.repositories, r.id]
                              : profile.repositories.filter(
                                  (id) => id !== r.id,
                                ),
                          })
                        }
                      />
                      <span>{folderName(r.root)}</span>
                      {terms !== undefined && (
                        <span className="rmeta">
                          {terms.toLocaleString("en-US")} terms
                        </span>
                      )}
                    </label>
                  );
                })}
              </div>
            )}
          </div>
          <div className="card rows">
            <Row
              title="Auto-accept"
              note={
                profile.auto_accept
                  ? "Conch replaces a heard phrase with the identifier it sounds like when no other suggestion competes for those words. Each replacement is listed under Accepted edits."
                  : "Repository suggestions wait for you to accept them. Turn this on to apply an unambiguous match without review."
              }
            >
              <Toggle
                label="Auto-accept repository suggestions"
                checked={profile.auto_accept}
                onChange={(auto_accept) => update({ auto_accept })}
              />
            </Row>
          </div>
          <div className="actions profile-actions">
            {profile.id !== active && (
              <button
                onClick={() =>
                  onChange({ ...settings, active_profile: profile.id })
                }
              >
                Make active
              </button>
            )}
            <button onClick={() => add(profile, `${profile.name} copy`)}>
              Duplicate
            </button>
            <span className="spacer" />
            {settings.profiles.length > 1 && (
              <button className="ghost danger-text" onClick={remove}>
                Delete profile
              </button>
            )}
          </div>
        </div>
      </div>
    </>
  );
}
