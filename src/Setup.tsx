import { PasteAccess } from "./PasteAccess";
import { Check } from "./icons";
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
  modelSize = "78 MB",
  voiceOnly = false,
  onDownload,
  downloading = false,
  progress = "",
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
  modelSize?: string;
  /** The recognizer is installed and only the voice detection model is missing. */
  voiceOnly?: boolean;
  onDownload: () => void;
  downloading?: boolean;
  progress?: string;
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
      <div className="step">
        <Step done={installed} number={1} />
        <div>
          <h3>English recognition</h3>
          <p>
            {installed
              ? "Installed. Speech stays on this device."
              : voiceOnly
                ? "Conch now also needs a 0.9 MB voice detection model so silence is not turned into words. No account or payment required."
                : `Download the ${modelSize} speech model. No account or payment required.`}
          </p>
        </div>
        {!installed && (
          <button
            className="primary"
            disabled={downloading}
            onClick={onDownload}
          >
            {downloading
              ? `Downloading ${progress}`
              : "Download recognition model"}
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
