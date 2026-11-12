import { PasteAccess } from "./PasteAccess";
import { Check } from "./icons";
import {
  DownloadProgress,
  RECOGNIZER_TRADEOFF,
  LanguageMenu,
  RecognizerMenu,
  voiceDetectionNote,
} from "./ModelChoice";
import { recognizerInfo, size, voiceDetection } from "./models";
import type { Language, ModelView } from "./types";
import { useEffect, useRef, useState } from "react";

export function shortcutLabel(value: string, mac = false) {
  return value
    .replace(/CommandOrControl/g, mac ? "Command" : "Ctrl")
    .replace(/Control/g, "Ctrl")
    .replace(/Meta|Super/g, mac ? "Command" : "Super")
    .replace(/Key([A-Z])/g, "$1")
    .replace(/Digit([0-9])/g, "$1")
    .replace(/\+/g, " + ");
}
export function Setup({
  installed,
  models,
  languages,
  language,
  onLanguage,
  recognizer,
  onRecognizer,
  ready,
  voiceOnly = false,
  onDownload,
  downloading = false,
  otherDownload = false,
  percent = null,
  accessibility = true,
  onPermission,
  onMicrophone,
  microphone = "unknown",
  hotkey,
  onSettings,
  toggle = false,
  clipboardOnly = false,
  autoPaste = true,
  mac = false,
  onRepair,
  onRestart,
  installationPath,
  busy = false,
}: {
  installed: boolean;
  /** The model catalog from `view`. */
  models: ModelView[];
  /** Every recognition language, from `view`. */
  languages: Language[];
  /** The saved recognition language. */
  language: string;
  /** Saves a different recognition language. */
  onLanguage?: (code: string) => void;
  /** The saved recognition model. */
  recognizer: string;
  /** Saves a different recognition model. */
  onRecognizer?: (id: string) => void;
  /** Whether a model can record now. Defaults to the saved model alone. */
  ready?: (id: string) => boolean;
  /** The recognizer is installed and only the voice detection model is missing. */
  voiceOnly?: boolean;
  /** Downloads the saved recognition model. */
  onDownload: () => void;
  /** The saved recognition model is downloading. */
  downloading?: boolean;
  /** A different download is running. */
  otherDownload?: boolean;
  /** Download progress, or null before the first report. */
  percent?: number | null;
  accessibility?: boolean;
  onPermission?: () => void;
  onMicrophone?: () => void;
  microphone?: string;
  hotkey?: string;
  onSettings?: () => void;
  toggle?: boolean;
  clipboardOnly?: boolean;
  /** Off copies the result without pasting, so no paste permission is needed. */
  autoPaste?: boolean;
  mac?: boolean;
  onRepair?: () => Promise<boolean>;
  onRestart?: () => void;
  installationPath?: string | null;
  busy?: boolean;
}) {
  const [changing, setChanging] = useState(false);
  const model = recognizerInfo(models, recognizer);
  const vad = voiceDetection(models);
  const isReady = ready ?? ((id: string) => installed && id === recognizer);
  const microphoneReady = microphone === "system" || microphone === "granted";
  const copyOnly = clipboardOnly || !autoPaste;
  const pasteReady = copyOnly || accessibility;
  return (
    <section className="card setup" aria-label="Dictation setup">
      <h2>
        {installed
          ? "Your dictation setup"
          : "One download, then you can dictate"}
      </h2>
      <div className={`step model${installed && !changing ? " settled" : ""}`}>
        <Step done={installed} number={1} />
        <div>
          <h3>Speech recognition</h3>
          {installed && !changing ? (
            <p>{model.label} is installed. Speech stays on this device.</p>
          ) : (
            <>
              <p>
                {voiceOnly
                  ? `Conch now also needs a ${size(vad)} voice detection model so silence is not turned into words. No account or payment required.`
                  : "Choose a speech model to download once. No account or payment required."}
              </p>
              <div className="picker">
                <LanguageMenu
                  languages={languages}
                  models={models}
                  value={language}
                  disabled={downloading}
                  onChange={(code) => {
                    onLanguage?.(code);
                    setChanging(false);
                  }}
                />
                <RecognizerMenu
                  models={models}
                  language={language}
                  value={recognizer}
                  disabled={downloading}
                  ready={isReady}
                  onChange={(id) => {
                    onRecognizer?.(id);
                    setChanging(false);
                  }}
                />
                {!installed && !downloading && (
                  <button
                    className="primary"
                    disabled={otherDownload}
                    onClick={onDownload}
                  >
                    {voiceOnly
                      ? `Download ${vad.label}, ${size(vad)}`
                      : `Download ${model.label}, ${size(model)}`}
                  </button>
                )}
              </div>
              {downloading ? (
                <DownloadProgress label={model.label} percent={percent} />
              ) : (
                !installed && (
                  <p className="tradeoff">
                    {RECOGNIZER_TRADEOFF}
                    {voiceOnly ? "" : ` ${voiceDetectionNote(models)}`}
                  </p>
                )
              )}
            </>
          )}
        </div>
        {installed && !changing && (
          <button className="link" onClick={() => setChanging(true)}>
            Change model
          </button>
        )}
      </div>
      <div className="step">
        <Step done={microphoneReady} number={2} />
        <div>
          <h3>Microphone</h3>
          <p>
            {microphone === "system"
              ? "Your system manages microphone access. Check its privacy settings if recording fails."
              : microphone === "granted"
                ? "Microphone access is ready."
                : microphone === "denied"
                  ? "Allow Conch in your microphone privacy settings."
                  : "Allow microphone access before your first recording."}
          </p>
        </div>
        {!microphoneReady && onMicrophone && (
          <button onClick={onMicrophone}>Enable microphone</button>
        )}
      </div>
      <div className="step">
        <Step done={pasteReady} number={3} />
        <div>
          <h3>{copyOnly ? "Clipboard delivery" : "Paste into other apps"}</h3>
          {!copyOnly && mac && onRepair && onRestart && onPermission ? (
            <PasteAccess
              granted={accessibility}
              path={installationPath}
              busy={busy}
              onOpen={onPermission}
              onRepair={onRepair}
              onRestart={onRestart}
            />
          ) : (
            <p>
              {!autoPaste
                ? "Automatic paste is off in Settings. Conch copies your text; paste it manually into the receiving app."
                : clipboardOnly
                  ? "This desktop copies your text. Paste it manually into the receiving app."
                  : accessibility
                    ? "Automatic paste is enabled for supported text fields."
                    : "Enable Accessibility for Conch to paste after dictation. Clipboard copying works without it."}
            </p>
          )}
        </div>
        {!(mac && onRepair && onRestart && onPermission) &&
          !copyOnly &&
          !accessibility &&
          onPermission && (
            <button onClick={onPermission}>Enable automatic paste</button>
          )}
      </div>
      {hotkey && (
        <div className="howto">
          <kbd>{shortcutLabel(hotkey, mac)}</kbd>
          <span>
            {toggle
              ? "Focus a text field in another app. Press the shortcut to start, then press again to finish."
              : "Focus a text field in another app, hold to speak, then release."}
          </span>
          <button className="ghost" onClick={onSettings}>
            Change shortcut
          </button>
        </div>
      )}
      <p className="note">
        Repository context and Enhanced cleanup are optional. Basic cleanup
        removes your hesitation tokens and needs no model.
      </p>
    </section>
  );
}
function Step({ done, number }: { done: boolean; number: number }) {
  return (
    <span className={`step-mark${done ? " done" : ""}`} aria-hidden="true">
      {done ? <Check /> : number}
    </span>
  );
}
export function ShortcutRecorder({
  value,
  onChange,
  onCapture,
  mac = false,
}: {
  value: string;
  onChange: (value: string) => void | Promise<void>;
  onCapture?: (active: boolean) => Promise<void>;
  mac?: boolean;
}) {
  const [capturing, setCapturing] = useState(false);
  const [preview, setPreview] = useState("");
  const chord = useRef<string | undefined>(undefined);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const target = useRef<HTMLButtonElement>(null);
  const active = useRef(false);
  const mounted = useRef(true);
  const captureCallback = useRef(onCapture);
  captureCallback.current = onCapture;
  useEffect(() => {
    if (capturing) target.current?.focus();
  }, [capturing]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (active.current) {
        active.current = false;
        void captureCallback.current?.(false).catch(() => {});
      }
    };
  }, []);
  async function finish(next?: string) {
    active.current = false;
    chord.current = undefined;
    setPreview("");
    setCapturing(false);
    try {
      await onCapture?.(false);
      if (next) await onChange(next);
    } catch (e) {
      setError(String(e));
    }
  }
  return (
    <div className="shortcut-control">
      <kbd>{shortcutLabel(value, mac)}</kbd>
      <button
        ref={target}
        disabled={pending}
        aria-label={capturing ? "Press your shortcut" : "Change shortcut"}
        className={capturing ? "capture-active" : ""}
        onClick={async () => {
          if (capturing) return;
          setPending(true);
          setError("");
          try {
            await onCapture?.(true);
            if (!mounted.current) {
              await onCapture?.(false);
              return;
            }
            active.current = true;
            setCapturing(true);
          } catch (e) {
            setError(String(e));
          } finally {
            if (mounted.current) setPending(false);
          }
        }}
        onBlur={() => {
          if (capturing) void finish();
        }}
        onKeyDown={(e) => {
          if (!capturing) return;
          e.preventDefault();
          e.stopPropagation();
          if (e.repeat) return;
          if (e.code === "Escape") {
            void finish();
            return;
          }
          if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
          if (!(e.ctrlKey || e.metaKey || e.altKey)) {
            setError("Include Command, Control, or Alt with a key.");
            return;
          }
          const modifiers = [
            e.metaKey ? "Super" : "",
            e.ctrlKey ? "Control" : "",
            e.altKey ? "Alt" : "",
            e.shiftKey ? "Shift" : "",
          ].filter(Boolean);
          chord.current = [...modifiers, e.code].join("+");
          setPreview(chord.current);
        }}
        onKeyUp={(e) => {
          if (
            capturing &&
            chord.current &&
            !e.ctrlKey &&
            !e.metaKey &&
            !e.altKey &&
            !e.shiftKey
          ) {
            e.preventDefault();
            void finish(chord.current);
          }
        }}
      >
        {pending
          ? "Preparing..."
          : capturing
            ? preview
              ? `Release ${shortcutLabel(preview, mac)} to use it`
              : "Press your shortcut - Esc to cancel"
            : "Change shortcut"}
      </button>
      {capturing && (
        <small>Recording shortcuts are paused while you choose.</small>
      )}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
    </div>
  );
}
