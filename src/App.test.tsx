import { afterEach, beforeEach, it, expect, vi } from "vitest";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
} from "@testing-library/react";
import App from "./App";
import { IndicatorView } from "./RecordingIndicator";
const ipc = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
const fixture = () => ({
  settings: {
    repositories: [],
    mappings: [],
    hotkey: "Control+Space",
    activation: "push",
    show_overlay: true,
    cleanup: {
      mode: "basic",
      fillers: ["um", "uh"],
      excluded: [],
      provider: "managed",
      endpoint: "",
      model: "",
      remote_enabled: false,
    },
  },
  phase: "idle",
  result: null as null | ReturnType<typeof suggestionResult>,
  error: "",
  indexes: [],
  models: [
    ["ggml-tiny.en.bin", false],
    ["s1-mini-q4_k_m.gguf", false],
  ],
  accessibility: false,
  microphone: "unknown",
  platform: "macos",
  downloading: false,
  can_retry: false,
  shortcuts_available: true,
});
let view = fixture();
beforeEach(() => {
  view = fixture();
  ipc.mockReset();
  ipc.mockImplementation(
    async (name: string, args?: Record<string, unknown>) => {
      if (name === "view") return structuredClone(view);
      if (name === "download") {
        view.models[0][1] = true;
        return;
      }
      if (name === "save_settings") {
        view.settings = args?.settings as typeof view.settings;
      }
      return;
    },
  );
});
afterEach(cleanup);
it("starts model setup directly from the dictation page and enables recording on completion", async () => {
  view.microphone = "granted";
  render(<App />);
  const download = await screen.findByRole("button", {
    name: /download recognition model/i,
  });
  expect(
    (
      screen.getByRole("button", {
        name: "Start recording",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  fireEvent.click(download);
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("download", { model: 0 }),
  );
  await waitFor(() =>
    expect(
      (
        screen.getByRole("button", {
          name: "Start recording",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false),
  );
  expect(
    ipc.mock.calls.some(
      ([name, args]) => name === "download" && args?.model === 1,
    ),
  ).toBe(false);
});
it("offers automatic-paste permission from setup", async () => {
  render(<App />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Enable automatic paste" }),
  );
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("request_accessibility", undefined),
  );
});
it("pauses the registered shortcut during capture and saves a detected chord", async () => {
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
  fireEvent.click(screen.getByRole("button", { name: "Change shortcut" }));
  const capture = await screen.findByRole("button", {
    name: "Press your shortcut",
  });
  expect(ipc).toHaveBeenCalledWith("shortcut_capture", { active: true });
  fireEvent.keyDown(capture, {
    key: "k",
    code: "KeyK",
    ctrlKey: true,
    altKey: true,
  });
  fireEvent.keyUp(capture, { key: "Alt", code: "AltLeft" });
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("shortcut_capture", { active: false }),
  );
  fireEvent.click(await screen.findByRole("button", { name: "Save changes" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ hotkey: "Control+Alt+KeyK" }),
    }),
  );
});
it("shows recording progress without interactive controls in the overlay", () => {
  render(
    <IndicatorView
      state={{
        visible: true,
        label: "Listening",
        hint: "Release shortcut to finish",
        level: 0.8,
        seconds: 65,
      }}
    />,
  );
  expect(screen.getByRole("status").textContent).toContain("1:05");
  expect(screen.queryAllByRole("button")).toHaveLength(0);
});

it("shows clipboard fallback on Linux settings instead of an unsupported permission action", async () => {
  view.platform = "linux";
  view.microphone = "system";
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
  expect(
    screen.queryByRole("button", { name: "Open paste permissions" }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Check microphone access" }),
  ).toBeNull();
  expect(screen.getByText(/paste manually/i)).toBeTruthy();
});

it("activates a captured shortcut immediately without saving unrelated settings", async () => {
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
  fireEvent.change(screen.getByLabelText("Activation"), {
    target: { value: "toggle" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Change shortcut" }));
  const capture = await screen.findByRole("button", {
    name: "Press your shortcut",
  });
  fireEvent.keyDown(capture, {
    key: "j",
    code: "KeyJ",
    ctrlKey: true,
    shiftKey: true,
  });
  fireEvent.keyUp(capture, { key: "Shift", code: "ShiftLeft" });
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("set_hotkey", {
      hotkey: "Control+Shift+KeyJ",
    }),
  );
  expect(ipc.mock.calls.some(([name]) => name === "save_settings")).toBe(false);
});
const suggestionResult = (conflict: boolean) => ({
  raw: "raise max group",
  text: "raise max group",
  basic: "raise max group",
  status: "Prepared locally",
  edits: [],
  suggestions: [
    {
      start: 6,
      end: 15,
      original: "max group",
      conflict,
      candidates: [{ text: "maxGroups", repository: "r", file: "a.ts" }],
    },
  ],
});
it("offers to remember an accepted repository suggestion as a mapping", async () => {
  view.result = suggestionResult(false);
  render(<App />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Accept and always replace" }),
  );
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("accept_always", {
      index: 0,
      candidate: 0,
    }),
  );
});
it("does not offer to remember a mapping conflict", async () => {
  view.result = suggestionResult(true);
  render(<App />);
  await screen.findByRole("button", { name: "Accept" });
  expect(
    screen.queryByRole("button", { name: "Accept and always replace" }),
  ).toBeNull();
});
