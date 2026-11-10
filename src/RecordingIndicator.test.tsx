import { it, expect, afterEach } from "vitest";
import { render, cleanup } from "@testing-library/react";
import { IndicatorView, type IndicatorState } from "./RecordingIndicator";
afterEach(cleanup);
const base: IndicatorState = {
  visible: true,
  mode: "recording",
  label: "Listening",
  hint: "",
  level: 0,
  seconds: 0,
};
it("shows the audio bars while recording", () => {
  const { container } = render(<IndicatorView state={base} />);
  expect(container.querySelector(".audio-bars")).not.toBeNull();
  expect(container.querySelector(".spiral")).toBeNull();
});
it("shows the spinner while processing", () => {
  const { container } = render(
    <IndicatorView state={{ ...base, mode: "processing" }} />,
  );
  expect(container.querySelector(".spiral")).not.toBeNull();
});
it("renders neither animation while hidden, so a stale mode cannot flash on show", () => {
  const { container } = render(
    <IndicatorView state={{ ...base, visible: false, mode: "processing" }} />,
  );
  expect(container.querySelector(".spiral")).toBeNull();
  expect(container.querySelector(".audio-bars")).toBeNull();
  expect(container.querySelector("time")).toBeNull();
});
