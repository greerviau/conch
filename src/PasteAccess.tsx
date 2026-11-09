import { useState } from "react";
export type PasteAccessProps = {
  granted: boolean;
  path?: string | null;
  busy?: boolean;
  onOpen: () => void;
  onRepair: () => Promise<boolean>;
  onRestart: () => void;
};
export function PasteAccess({
  granted,
  path,
  busy = false,
  onOpen,
  onRepair,
  onRestart,
}: PasteAccessProps) {
  const [confirm, setConfirm] = useState(false);
  const [repairing, setRepairing] = useState(false);
  if (granted)
    return (
      <p role="status">Automatic paste is enabled for supported text fields.</p>
    );
  return (
    <div className="paste-access">
      <p>
        macOS has not authorized this running copy of Conch. Clipboard copying
        still works.
      </p>
      <button onClick={onOpen}>Enable automatic paste</button>
      <details>
        <summary>Enabled in Settings but still not detected?</summary>
        <p>
          An app update can leave an enabled Accessibility entry attached to an
          older build. Remove that entry and authorize the installed copy again.
        </p>
        {path && (
          <p>
            Running copy: <code>{path}</code>
          </p>
        )}
        {!confirm ? (
          <button disabled={busy || repairing} onClick={() => setConfirm(true)}>
            Repair Accessibility entry
          </button>
        ) : (
          <div className="permission-repair">
            <p>
              This removes only Conch's Accessibility entry. You must enable
              Conch again in System Settings.
            </p>
            <div className="actions">
              <button
                disabled={busy || repairing}
                onClick={async () => {
                  setRepairing(true);
                  try {
                    if (await onRepair()) setConfirm(false);
                  } finally {
                    setRepairing(false);
                  }
                }}
              >
                {repairing
                  ? "Removing old entry..."
                  : "Remove old entry and open Settings"}
              </button>
              <button disabled={repairing} onClick={() => setConfirm(false)}>
                Cancel
              </button>
            </div>
          </div>
        )}
        <p>
          After enabling the current copy, return to Conch. If it still reports
          missing access, restart Conch.
        </p>
        <p className="muted">
          Restart clears the in-app result and any recording kept for Retry.
          Copied text remains on the clipboard.
        </p>
        <button disabled={busy || repairing} onClick={onRestart}>
          Restart Conch
        </button>
      </details>
    </div>
  );
}
