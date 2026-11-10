import { Check } from "./icons";
import { RECOGNIZERS, VAD, size } from "./models";

export const RECOGNIZER_TRADEOFF =
  "Larger models make fewer errors and take longer to finish after you stop speaking.";
export const VOICE_DETECTION_NOTE = `Also installs the ${size(VAD)} voice detection model.`;

export const recognizerInfo = (id: string) =>
  RECOGNIZERS.find((model) => model.id === id) ?? RECOGNIZERS[0];

/** Every recognition model. An installed one shows Installed where the others show a size. */
export function RecognizerMenu({
  value,
  onChange,
  ready,
  disabled = false,
}: {
  value: string;
  onChange: (id: string) => void;
  ready: (id: string) => boolean;
  disabled?: boolean;
}) {
  return (
    <select
      aria-label="Recognition model"
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
    >
      {RECOGNIZERS.map((model) => (
        <option key={model.id} value={model.id}>
          {model.label} - {ready(model.id) ? "Installed" : size(model)} -{" "}
          {model.note}
        </option>
      ))}
    </select>
  );
}

/** A null percent means the download has started but reported no size yet. */
export function DownloadProgress({
  label,
  percent,
}: {
  label: string;
  percent: number | null;
}) {
  return (
    <div className="progress">
      <div
        className="bar"
        role="progressbar"
        aria-label="Download progress"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
      >
        <i
          className={percent === null ? "indeterminate" : undefined}
          style={percent === null ? undefined : { width: `${percent}%` }}
        />
      </div>
      <small>
        {percent === null
          ? `Downloading ${label}`
          : `Downloading ${label}, ${percent}%`}
      </small>
    </div>
  );
}

/** Installed, downloadable, or downloading: the state of one model, shown beside the control that picks it. */
export function ModelStatus({
  ready,
  downloading,
  percent,
  blocked,
  label,
  readyText,
  needText,
  button,
  note,
  onDownload,
}: {
  ready: boolean;
  downloading: boolean;
  percent: number | null;
  /** Another download is running. */
  blocked: boolean;
  label: string;
  readyText: string;
  needText: string;
  button: string;
  note?: string;
  onDownload: () => void;
}) {
  if (downloading)
    return (
      <div className="status">
        <DownloadProgress label={label} percent={percent} />
      </div>
    );
  if (ready)
    return (
      <div className="status">
        <div className="line">
          <span className="tick" aria-hidden="true">
            <Check />
          </span>
          {readyText}
        </div>
      </div>
    );
  return (
    <div className="status">
      <p>{needText}</p>
      <div className="act">
        <button className="primary" disabled={blocked} onClick={onDownload}>
          {button}
        </button>
        {note && <small>{note}</small>}
      </div>
    </div>
  );
}
