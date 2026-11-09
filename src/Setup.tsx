import { PasteAccess } from "./PasteAccess";
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
  mac = false,
  onRepair,
  onRestart,
  installationPath,
  busy = false,
}: {
  installed: boolean;
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
  mac?: boolean;
  onRepair?: () => Promise<boolean>;
  onRestart?: () => void;
  installationPath?: string | null;
  busy?: boolean;
}) {
  return (
    <section className="setup-card" aria-label="Dictation setup">
      <div className="section-title">
        <div>
          <p className="eyebrow">GET STARTED</p>
          <h2>
            {installed
              ? "Your dictation setup"
              : "One download, then you can dictate"}
          </h2>
        </div>
        <span className="badge">
          {installed ? "Model installed" : "Setup needed"}
        </span>
      </div>
      <div className="setup-step">
        <span className="step-number">1</span>
        <div>
          <h3>English recognition</h3>
          <p>
            {installed
              ? "Installed. Speech stays on this device."
              : "Download the 78 MB speech model. No account or payment required."}
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
      <div className="setup-step">
        <span className="step-number">2</span>
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
        {microphone !== "system" &&
          microphone !== "granted" &&
          onMicrophone && (
            <button onClick={onMicrophone}>Enable microphone</button>
          )}
      </div>
      <div className="setup-step">
        <span className="step-number">3</span>
        <div>
          <h3>
            {clipboardOnly ? "Clipboard delivery" : "Paste into other apps"}
          </h3>
          {mac && onRepair && onRestart && onPermission ? (
            <PasteAccess
              granted={accessibility}
              path={installationPath}
              busy={busy}
              onOpen={onPermission}
              onRepair={onRepair}
              onRestart={onRestart}
            />
          ) : (
            <>
              <p>
                {clipboardOnly
                  ? "This desktop copies your text. Paste it manually into the receiving app."
                  : accessibility
                    ? "Automatic paste is enabled for supported text fields."
                    : "Enable Accessibility for Conch to paste after dictation. Clipboard copying works without it."}
              </p>
              {!clipboardOnly && !accessibility && onPermission && (
                <button onClick={onPermission}>Enable automatic paste</button>
              )}
            </>
          )}
        </div>
      </div>
      {hotkey && (
        <div className="setup-howto">
          <kbd>{shortcutLabel(hotkey, mac)}</kbd>
          <span>
            {toggle
              ? "Focus a text field in another app. Press the shortcut to start, then press again to finish."
              : "Focus a text field in another app, hold to speak, then release."}
          </span>
          <button onClick={onSettings}>Change shortcut</button>
        </div>
      )}
      <p className="muted">
        Repository context and Enhanced cleanup are optional. Start with Basic
        cleanup.
      </p>
    </section>
  );
}
export function ShortcutRecorder({
  value,
  onChange,
  onCapture,
  mac = false,
}: {
  value: string;
  onChange: (value: string) => void;
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
      if (next) onChange(next);
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
      <small>
        {capturing
          ? "Recording shortcuts are paused while you choose."
          : "Click Change shortcut, then press your preferred key combination."}
      </small>
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
    </div>
  );
}
