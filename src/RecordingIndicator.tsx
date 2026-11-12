import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { Spiral } from "./Spiral";
export type IndicatorState = {
  visible: boolean;
  mode: "recording" | "processing" | "notice";
  label: string;
  hint: string;
  level: number;
  seconds: number;
  /** The active profile while recording with more than one; empty otherwise. */
  profile: string;
  /** Listed while the profile list is open. */
  profiles: {
    id: string;
    name: string;
    active: boolean;
    /** Whether its recognition model is installed. */
    available: boolean;
  }[];
};
function ProfileIcon() {
  return (
    <svg
      className="profile-icon"
      width="13"
      height="13"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      aria-hidden="true"
    >
      <circle cx="8" cy="5.5" r="2.75" />
      <path d="M2.75 14c.6-2.9 2.7-4.5 5.25-4.5s4.65 1.6 5.25 4.5" />
    </svg>
  );
}
export function IndicatorView({
  state,
  onToggleList = () => {},
  onChoose = () => {},
}: {
  state: IndicatorState;
  onToggleList?: (open: boolean) => void;
  onChoose?: (id: string) => void;
}) {
  // The window is reused between dictations. Rendering nothing while hidden
  // keeps the previous mode from showing before the first event of the next one.
  if (!state.visible) return null;
  const time = `${Math.floor(state.seconds / 60)}:${String(state.seconds % 60).padStart(2, "0")}`;
  const open = state.profiles.length > 0;
  return (
    <div
      className="recording-indicator"
      data-mode={state.mode}
      data-open={open || undefined}
    >
      {open && (
        <div className="indicator-profiles" role="menu" aria-label="Profile">
          {state.profiles.map((p) => (
            <button
              key={p.id}
              role="menuitemradio"
              aria-checked={p.active}
              className={p.active ? "active" : undefined}
              disabled={!p.available}
              onClick={() => onChoose(p.id)}
            >
              {p.name}
            </button>
          ))}
        </div>
      )}
      <div
        className="indicator-row"
        role="status"
        aria-live="polite"
        data-mode={state.mode}
      >
        {state.mode === "recording" && (
          <div className="audio-bars" aria-hidden="true">
            {[0.5, 0.85, 1, 0.7, 0.45].map((v, i) => (
              <span
                key={i}
                style={{ height: `${5 + Math.min(1, state.level) * 18 * v}px` }}
              />
            ))}
          </div>
        )}
        {state.mode === "processing" && <Spiral />}
        {state.mode === "notice" ? (
          <span className="notice-text">{state.label}</span>
        ) : (
          <>
            <span className="sr-only">{state.label}</span>
            <time>{time}</time>
          </>
        )}
        {state.mode === "recording" && state.profile && (
          <button
            className="indicator-profile"
            aria-haspopup="menu"
            aria-expanded={open}
            aria-label={`Profile ${state.profile}`}
            onClick={() => onToggleList(!open)}
          >
            <ProfileIcon />
            <span>{state.profile}</span>
          </button>
        )}
      </div>
    </div>
  );
}
export function RecordingIndicator() {
  const [state, setState] = useState<IndicatorState>({
    visible: false,
    mode: "recording",
    label: "",
    hint: "",
    level: 0,
    seconds: 0,
    profile: "",
    profiles: [],
  });
  useEffect(() => {
    let disposed = false;
    const subscription = listen<IndicatorState>("recording-state", (e) => {
      if (!disposed) setState(e.payload);
    });
    void invoke<IndicatorState>("indicator_state").then((s) => {
      if (!disposed) setState(s);
    });
    return () => {
      disposed = true;
      void subscription.then((f) => f());
    };
  }, []);
  return (
    <IndicatorView
      state={state}
      onToggleList={(open) => void invoke("indicator_menu", { open })}
      onChoose={(id) =>
        void invoke("choose_profile", { id, fromIndicator: true }).catch(
          () => undefined,
        )
      }
    />
  );
}
