import { it, expect, afterEach, vi } from "vitest";
import { render, cleanup, fireEvent, screen } from "@testing-library/react";
import { IndicatorView, type IndicatorState } from "./RecordingIndicator";
afterEach(cleanup);
const base: IndicatorState = {
  visible: true,
  mode: "recording",
  label: "Listening",
  hint: "",
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
