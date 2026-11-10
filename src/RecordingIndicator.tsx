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
};
export function IndicatorView({ state }: { state: IndicatorState }) {
  // The window is reused between dictations. Rendering nothing while hidden
  // keeps the previous mode from showing before the first event of the next one.
  if (!state.visible) return null;
  const time = `${Math.floor(state.seconds / 60)}:${String(state.seconds % 60).padStart(2, "0")}`;
  return (
    <div
      className="recording-indicator"
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
  return <IndicatorView state={state} />;
}
