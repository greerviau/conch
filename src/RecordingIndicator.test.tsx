import { it, expect, afterEach, vi } from "vitest";
import {
  render,
  cleanup,
  fireEvent,
  screen,
  act,
} from "@testing-library/react";
import {
  IndicatorView,
  RecordingIndicator,
  type IndicatorState,
} from "./RecordingIndicator";
const ipc = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc }));
const events = vi.hoisted(() => ({
  state: null as null | ((event: { payload: unknown }) => void),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    async (name: string, callback: (event: { payload: unknown }) => void) => {
      if (name === "recording-state") events.state = callback;
      return () => {};
    },
  ),
}));
afterEach(cleanup);
const base: IndicatorState = {
  visible: true,
  mode: "recording",
  label: "Listening",
  hint: "",
  starting: false,
  level: 0,
  seconds: 0,
  profile: "",
  profiles: [],
};
it("shows the audio bars while recording", () => {
  const { container } = render(<IndicatorView state={base} />);
  expect(container.querySelector(".audio-bars")).not.toBeNull();
  expect(container.querySelector(".spiral")).toBeNull();
});
it("marks a recording that has no audio yet as starting, and drops the mark when audio arrives", () => {
  const { container, rerender } = render(
    <IndicatorView state={{ ...base, starting: true }} />,
  );
  const pill = container.querySelector(".recording-indicator")!;
  expect(pill.getAttribute("data-starting")).toBe("true");
  expect(container.querySelector(".audio-bars")).not.toBeNull();
  expect(container.querySelector("time")?.textContent).toBe("0:00");
  rerender(<IndicatorView state={base} />);
  expect(pill.hasAttribute("data-starting")).toBe(false);
});
it("shows the spinner while processing", () => {
  const { container } = render(
    <IndicatorView state={{ ...base, mode: "processing" }} />,
  );
  expect(container.querySelector(".spiral")).not.toBeNull();
});
it("renders the next recording's pill while hidden, so the window never shows an empty frame", () => {
  const { container } = render(
    <IndicatorView
      state={{ ...base, visible: false, mode: "recording", seconds: 0 }}
    />,
  );
  expect(container.querySelector(".recording-indicator")).not.toBeNull();
  expect(container.querySelector(".audio-bars")).not.toBeNull();
  expect(container.querySelector(".spiral")).toBeNull();
  expect(container.querySelector("time")?.textContent).toBe("0:00");
});
it("shows no profile with one profile, so the pill is the bars and the time", () => {
  const { container } = render(<IndicatorView state={base} />);
  expect(container.querySelector(".indicator-profile")).toBeNull();
  expect(container.querySelector("button")).toBeNull();
});
it("names the active profile with an icon after the time and opens the list on click", () => {
  const toggle = vi.fn();
  const { container } = render(
    <IndicatorView
      state={{ ...base, profile: "conch" }}
      onToggleList={toggle}
    />,
  );
  const button = screen.getByRole("button", { name: "Profile conch" });
  const row = container.querySelector(".indicator-row")!;
  const order = Array.from(row.children).map((child) => child.tagName);
  expect(order.indexOf("TIME")).toBeLessThan(order.indexOf("BUTTON"));
  expect(button.querySelector("svg.profile-icon")).not.toBeNull();
  expect(button.textContent).toBe("conch");
  expect(screen.queryByRole("menu")).toBeNull();
  fireEvent.click(button);
  expect(toggle).toHaveBeenCalledWith(true);
});
it("lists the profiles while open, marks the active one, and disables one without its model", () => {
  const choose = vi.fn();
  render(
    <IndicatorView
      state={{
        ...base,
        profile: "conch",
        profiles: [
          { id: "w", name: "Writing", active: false, available: true },
          { id: "c", name: "conch", active: true, available: true },
          { id: "l", name: "Large", active: false, available: false },
        ],
      }}
      onChoose={choose}
    />,
  );
  const items = screen.getAllByRole("menuitemradio");
  expect(items.map((item) => item.getAttribute("aria-checked"))).toEqual([
    "false",
    "true",
    "false",
  ]);
  expect((items[2] as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(items[0]);
  expect(choose).toHaveBeenCalledWith("w");
});
it("shows no profile while processing", () => {
  const { container } = render(
    <IndicatorView state={{ ...base, mode: "processing", profile: "conch" }} />,
  );
  expect(container.querySelector(".indicator-profile")).toBeNull();
});
it("keeps an event that arrives before the initial state, since the backend sends only changes", async () => {
  let answer: (state: IndicatorState) => void = () => {};
  ipc.mockImplementation(
    () => new Promise<IndicatorState>((resolve) => (answer = resolve)),
  );
  const { container } = render(<RecordingIndicator />);
  await act(async () => {});
  act(() => events.state!({ payload: { ...base, mode: "processing" } }));
  await act(async () => answer({ ...base, visible: false }));
  expect(container.querySelector(".spiral")).not.toBeNull();
});
