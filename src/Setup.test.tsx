import { afterEach, describe, it, expect, vi } from "vitest";
import {
  render,
  screen,
  cleanup,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { Setup, ShortcutRecorder } from "./Setup";
afterEach(cleanup);
describe("first-run setup", () => {
  it("offers the required download where dictation starts", () => {
    const download = vi.fn();
    render(<Setup installed={false} onDownload={download} />);
    fireEvent.click(
      screen.getByRole("button", { name: /download.*recognition/i }),
    );
    expect(download).toHaveBeenCalledOnce();
  });
  it("does not ask users to install optional cleanup to dictate", () => {
    render(<Setup installed onDownload={vi.fn()} />);
    expect(
      screen.queryByRole("button", { name: /download.*recognition/i }),
    ).toBeNull();
  });
});
describe("paste step", () => {
  it("asks for the paste permission only while automatic paste is on", () => {
    const { rerender } = render(
      <Setup
        installed
        onDownload={vi.fn()}
        accessibility={false}
        onPermission={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Enable automatic paste" }),
    ).toBeTruthy();
    rerender(
      <Setup
        installed
        onDownload={vi.fn()}
        accessibility={false}
        autoPaste={false}
        onPermission={vi.fn()}
      />,
    );
    expect(
      screen.queryByRole("button", { name: "Enable automatic paste" }),
    ).toBeNull();
    expect(screen.getByText(/automatic paste is off/i)).toBeTruthy();
  });
});
describe("shortcut capture", () => {
  it("captures a chord and ignores key repeat without typing shortcut syntax", async () => {
    const change = vi.fn();
    render(<ShortcutRecorder value="Control+Space" onChange={change} />);
    fireEvent.click(screen.getByRole("button", { name: /change shortcut/i }));
    const capture = await screen.findByRole("button", {
      name: /press.*shortcut/i,
    });
    fireEvent.keyDown(capture, {
      key: "k",
      code: "KeyK",
      ctrlKey: true,
      shiftKey: true,
      repeat: true,
    });
    expect(change).not.toHaveBeenCalled();
    fireEvent.keyDown(capture, {
      key: "k",
      code: "KeyK",
      ctrlKey: true,
      shiftKey: true,
    });
    fireEvent.keyUp(capture, { key: "Control", code: "ControlLeft" });
    await waitFor(() =>
      expect(change).toHaveBeenCalledWith("Control+Shift+KeyK"),
    );
  });
  it("Escape cancels capture and modifier-only presses do not save", async () => {
    const change = vi.fn();
    render(<ShortcutRecorder value="Control+Space" onChange={change} />);
    fireEvent.click(screen.getByRole("button", { name: /change shortcut/i }));
    const capture = await screen.findByRole("button", {
      name: /press.*shortcut/i,
    });
    fireEvent.keyDown(capture, {
      key: "Control",
      code: "ControlLeft",
      ctrlKey: true,
    });
    fireEvent.keyDown(capture, { key: "Escape", code: "Escape" });
    expect(change).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: /change shortcut/i }),
    ).toBeTruthy();
  });
});

it("restores the shortcut if the control unmounts while suspension is pending", async () => {
  let complete: () => void = () => {};
  const suspend = new Promise<void>((resolve) => {
    complete = resolve;
  });
  const capture = vi.fn((active: boolean) =>
    active ? suspend : Promise.resolve(),
  );
  const rendered = render(
    <ShortcutRecorder
      value="Control+Space"
      onChange={vi.fn()}
      onCapture={capture}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Change shortcut" }));
  rendered.unmount();
  complete();
  await waitFor(() => expect(capture).toHaveBeenCalledWith(false));
});

it("keeps the recording shortcut paused until the captured keys are released", async () => {
  const capture = vi.fn(async () => {});
  const change = vi.fn();
  render(
    <ShortcutRecorder
      value="Control+Space"
      onChange={change}
      onCapture={capture}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Change shortcut" }));
  const button = await screen.findByRole("button", {
    name: "Press your shortcut",
  });
  fireEvent.keyDown(button, { key: " ", code: "Space", ctrlKey: true });
  expect(capture).not.toHaveBeenCalledWith(false);
  fireEvent.keyUp(button, { key: " ", code: "Space", ctrlKey: true });
  expect(change).not.toHaveBeenCalled();
  fireEvent.keyUp(button, { key: "Control", code: "ControlLeft" });
  await waitFor(() => expect(change).toHaveBeenCalledWith("Control+Space"));
});

it("shows Command for the default shortcut on macOS setup", () => {
  render(
    <Setup
      installed
      onDownload={vi.fn()}
      hotkey="CommandOrControl+Shift+Space"
      mac
    />,
  );
  expect(screen.getByText("Command + Shift + Space")).toBeTruthy();
});
it("does not offer a no-op microphone permission button on system-managed platforms", () => {
  render(
    <Setup
      installed
      onDownload={vi.fn()}
      microphone="system"
      onMicrophone={vi.fn()}
    />,
  );
  expect(
    screen.queryByRole("button", { name: "Enable microphone" }),
  ).toBeNull();
  expect(screen.getByText(/system manages microphone/i)).toBeTruthy();
});
