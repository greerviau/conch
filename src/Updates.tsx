export type UpdateStatus = { state: string; detail?: string };

type Props = {
  version: string;
  update: UpdateStatus;
  busy: boolean;
  onCheck: () => void;
  onInstall: () => void;
};

function message(update: UpdateStatus) {
  switch (update.state) {
    case "checking":
      return "Checking for updates...";
    case "current":
      return "Conch is up to date.";
    case "available":
      return `Version ${update.detail} is available.`;
    case "installing":
      return "Installing the update. Conch restarts when it finishes.";
    case "failed":
      return update.detail ?? "The update failed.";
    default:
      return "";
  }
}

export function Updates({ version, update, busy, onCheck, onInstall }: Props) {
  const working = update.state === "checking" || update.state === "installing";
  return (
    <article>
      <h3>Updates</h3>
      <p>Conch {version}</p>
      {message(update) && <p>{message(update)}</p>}
      {update.state === "available" ? (
        <button disabled={busy} onClick={onInstall}>
          Install {update.detail}
        </button>
      ) : (
        <button disabled={working} onClick={onCheck}>
          Check for updates
        </button>
      )}
    </article>
  );
}
