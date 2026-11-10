import type { UpdateStatus } from "./types";
import { Row } from "./ui";

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
      return undefined;
  }
}

export function Updates({ version, update, busy, onCheck, onInstall }: Props) {
  const working = update.state === "checking" || update.state === "installing";
  return (
    <Row title={`Conch ${version}`} note={message(update)}>
      {update.state === "available" ? (
        <button disabled={busy} onClick={onInstall}>
          Install {update.detail}
        </button>
      ) : (
        <button disabled={working} onClick={onCheck}>
          Check for updates
        </button>
      )}
    </Row>
  );
}
