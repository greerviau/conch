import { afterEach, beforeEach, it, expect, vi } from "vitest";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
  act,
  within,
} from "@testing-library/react";
import App from "./App";
import { IndicatorView } from "./RecordingIndicator";
import { LANGUAGES, catalog } from "./testModels";
const ipc = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc }));
const events = vi.hoisted(() => ({
  progress: null as null | ((event: { payload: [number, number] }) => void),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    async (
      _name: string,
      callback: (event: { payload: [number, number] }) => void,
    ) => {
      events.progress = callback;
      return () => {};
    },
  ),
}));
const fixture = () => ({
  settings: {
    repositories: [],
    mappings: [],
    hotkey: "Control+Space",
    activation: "push",
    recognizer: "tiny.en",
    language: "en",
    auto_paste: true,
    auto_accept_suggestions: false,
    keep_history: false,
    microphone: "",
    show_overlay: true,
    check_updates: true,
    cleanup: {
      fillers: { en: ["um", "uh"] } as Record<string, string[]>,
      excluded: [],
      enhanced: "disabled",
      endpoint: "",
      model: "",
      remote_enabled: false,
    },
  },
  phase: "idle",
  result: null as null | ReturnType<typeof suggestionResult>,
  error: "",
  indexes: [],
  history_revision: 0,
  logs_revision: 0,
  models: catalog(),
  languages: LANGUAGES,
  accessibility: false,
  microphone: "unknown",
  platform: "macos",
  downloading: false,
  can_retry: false,
  shortcuts_available: true,
  version: "0.2.2",
  update: { state: "idle" } as { state: string; detail?: string },
});
let view = fixture();
let holdDownload: Promise<void> | null = null;
const install = (...ids: string[]) =>
  view.models.forEach((model) => {
    if (ids.includes(model.id)) model.ready = true;
  });
beforeEach(() => {
  view = fixture();
  holdDownload = null;
  events.progress = null;
  ipc.mockReset();
  ipc.mockImplementation(
    async (name: string, args?: Record<string, unknown>) => {
      if (name === "view") return structuredClone(view);
      if (name === "download") {
        // Downloading a recognizer also installs the voice detection model.
        const id = args?.id as string;
        await holdDownload;
        install(id, ...(id === "s1-mini" ? [] : ["silero-vad"]));
        return;
      }
      if (name === "logs_page")
        return { items: [], total: 0, all: 0, sources: [], bytes: 0 };
      if (name === "microphones")
        return ["Built-in Microphone", "USB Microphone"];
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
    name: "Download Tiny (English), 77.7 MB",
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
    expect(ipc).toHaveBeenCalledWith("download", { id: "tiny.en" }),
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
      ([name, args]) => name === "download" && args?.id === "s1-mini",
    ),
  ).toBe(false);
});
it("keeps recording off until the voice detection model is installed with the recognizer", async () => {
  view.microphone = "granted";
  install("tiny.en");
  render(<App />);
  expect(
    await screen.findByRole("button", {
      name: "Download Voice detection (Silero), 0.9 MB",
    }),
  ).toBeTruthy();
  expect(
    (
      screen.getByRole("button", {
        name: "Start recording",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
});
it("tells a copy that has the recognizer but not voice detection that one small file is missing", async () => {
  install("tiny.en");
  render(<App />);
  expect(await screen.findByText(/0\.9 MB voice detection model/)).toBeTruthy();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Download Voice detection (Silero), 0.9 MB",
    }),
  );
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("download", { id: "tiny.en" }),
  );
});
it("names the saved recognition model on the setup download", async () => {
  view.settings.recognizer = "small.en";
  render(<App />);
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Download Small (English), 487.6 MB",
    }),
  );
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("download", { id: "small.en" }),
  );
});
it("saves a recognition model picked on the setup card at once, without the Settings save bar", async () => {
  render(<App />);
  fireEvent.change(
    await screen.findByRole("combobox", { name: "Recognition model" }),
    { target: { value: "small.en" } },
  );
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ recognizer: "small.en" }),
    }),
  );
  expect(
    await screen.findByRole("button", {
      name: "Download Small (English), 487.6 MB",
    }),
  ).toBeTruthy();
  expect(screen.queryByText("Unsaved changes")).toBeNull();
});
it("keeps unsaved Settings edits when the setup card saves a recognition model", async () => {
  render(<App />);
  await openSettings();
  fireEvent.change(await screen.findByLabelText("Hesitation tokens"), {
    target: { value: "um, uh, er" },
  });
  fireEvent.click(await screen.findByRole("button", { name: "Dictation" }));
  fireEvent.change(
    await screen.findByRole("combobox", { name: "Recognition model" }),
    { target: { value: "base.en" } },
  );
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({
        recognizer: "base.en",
        cleanup: expect.objectContaining({ fillers: { en: ["um", "uh"] } }),
      }),
    }),
  );
  await openSettings();
  expect(
    ((await screen.findByLabelText("Hesitation tokens")) as HTMLInputElement)
      .value,
  ).toBe("um, uh, er");
  expect(
    (
      screen.getByRole("combobox", {
        name: "Recognition model",
      }) as HTMLSelectElement
    ).value,
  ).toBe("base.en");
});
it("lets the user choose a larger recognition model, download it, and save the choice", async () => {
  install("tiny.en", "silero-vad");
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
  const choice = (await screen.findByRole("combobox", {
    name: "Recognition model",
  })) as HTMLSelectElement;
  expect(choice.value).toBe("tiny.en");
  expect(Array.from(choice.options).map((option) => option.value)).toEqual([
    "tiny.en",
    "base.en",
    "small.en",
    "tiny",
    "base",
    "small",
    "large-v3-turbo",
  ]);
  fireEvent.change(choice, { target: { value: "small.en" } });
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Download Small (English), 487.6 MB",
    }),
  );
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("download", { id: "small.en" }),
  );
  fireEvent.click(await screen.findByRole("button", { name: "Save changes" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ recognizer: "small.en" }),
    }),
  );
});
it("offers no download for a recognition model that is already installed", async () => {
  install("tiny.en", "base.en", "silero-vad");
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
  fireEvent.change(
    await screen.findByRole("combobox", { name: "Recognition model" }),
    { target: { value: "base.en" } },
  );
  expect(screen.queryByRole("button", { name: /^Download Base/ })).toBeNull();
});
const openSettings = async () =>
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
const savedSettings = async () => {
  fireEvent.click(await screen.findByRole("button", { name: "Save changes" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("save_settings", expect.anything()),
  );
  const call = ipc.mock.calls.find(([name]) => name === "save_settings");
  return call?.[1].settings as typeof view.settings;
};
it("has no cleanup mode: Basic cleanup always runs and blank hesitation tokens remove nothing", async () => {
  render(<App />);
  await openSettings();
  expect(screen.queryByRole("combobox", { name: "Mode" })).toBeNull();
  expect(screen.getByLabelText("Hesitation tokens")).toBeTruthy();
  expect(screen.getByText(/leave blank to remove nothing/i)).toBeTruthy();
});
it("turns Enhanced cleanup off by default and offers the S1-mini download only for the managed choice", async () => {
  render(<App />);
  await openSettings();
  const choice = (await screen.findByRole("combobox", {
    name: "Enhanced cleanup",
  })) as HTMLSelectElement;
  expect(choice.value).toBe("disabled");
  expect(Array.from(choice.options).map((option) => option.value)).toEqual([
    "disabled",
    "managed",
    "local",
    "remote",
  ]);
  expect(
    screen.queryByRole("button", { name: /download s1-mini/i }),
  ).toBeNull();
  expect(screen.queryByLabelText("Endpoint")).toBeNull();
  fireEvent.change(choice, { target: { value: "managed" } });
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Download S1-mini by Superwhisper, 484.2 MB",
    }),
  );
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("download", { id: "s1-mini" }),
  );
  expect((await savedSettings()).cleanup.enhanced).toBe("managed");
});
it("asks for an endpoint when Enhanced cleanup uses a local or remote model", async () => {
  render(<App />);
  await openSettings();
  const choice = await screen.findByRole("combobox", {
    name: "Enhanced cleanup",
  });
  fireEvent.change(choice, { target: { value: "local" } });
  expect(await screen.findByLabelText("Endpoint")).toBeTruthy();
  expect(screen.queryByText(/dictated text leaves this device/i)).toBeNull();
  fireEvent.change(choice, { target: { value: "remote" } });
  expect(
    await screen.findByText(/dictated text leaves this device/i),
  ).toBeTruthy();
  fireEvent.change(choice, { target: { value: "disabled" } });
  expect(screen.queryByLabelText("Endpoint")).toBeNull();
});
it("follows the system microphone until the user picks a device, then saves its name", async () => {
  render(<App />);
  await openSettings();
  const choice = (await screen.findByRole("combobox", {
    name: "Microphone",
  })) as HTMLSelectElement;
  await waitFor(() => expect(choice.options.length).toBe(3));
  expect(choice.value).toBe("");
  expect(
    Array.from(choice.options).map((option) => option.textContent),
  ).toEqual(["System default", "Built-in Microphone", "USB Microphone"]);
  fireEvent.change(choice, { target: { value: "USB Microphone" } });
  expect((await savedSettings()).microphone).toBe("USB Microphone");
});
it("keeps a saved microphone in the list when it is unplugged", async () => {
  view.settings.microphone = "Studio Mic";
  render(<App />);
  await openSettings();
  const choice = (await screen.findByRole("combobox", {
    name: "Microphone",
  })) as HTMLSelectElement;
  expect(choice.value).toBe("Studio Mic");
  expect(screen.getByText(/not connected/i)).toBeTruthy();
});
it("pastes automatically by default and saves copy-only when the user turns it off", async () => {
  render(<App />);
  await openSettings();
  const toggle = (await screen.findByRole("checkbox", {
    name: "Automatic paste",
  })) as HTMLInputElement;
  expect(toggle.checked).toBe(true);
  fireEvent.click(toggle);
  expect(screen.getByText(/only copies the result/i)).toBeTruthy();
  expect((await savedSettings()).auto_paste).toBe(false);
});
it("skips the paste permission step in setup when automatic paste is off", async () => {
  view.settings.auto_paste = false;
  view.microphone = "granted";
  install("tiny.en", "silero-vad");
  render(<App />);
  expect(
    await screen.findByRole("button", { name: "Start recording" }),
  ).toBeTruthy();
  expect(screen.queryByRole("region", { name: "Dictation setup" })).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Enable automatic paste" }),
  ).toBeNull();
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
        mode: "recording",
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
it("saves the choice to stop checking for updates automatically", async () => {
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
  fireEvent.click(
    await screen.findByRole("checkbox", {
      name: "Check for updates automatically",
    }),
  );
  fireEvent.click(await screen.findByRole("button", { name: "Save changes" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ check_updates: false }),
    }),
  );
});
it("saves the Auto-accept choice", async () => {
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
  const toggle = await screen.findByRole("checkbox", {
    name: "Auto-accept repository suggestions",
  });
  expect((toggle as HTMLInputElement).checked).toBe(false);
  fireEvent.click(toggle);
  fireEvent.click(await screen.findByRole("button", { name: "Save changes" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ auto_accept_suggestions: true }),
    }),
  );
});
it("saves the Keep history choice", async () => {
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
  const toggle = await screen.findByRole("checkbox", {
    name: "Keep history of results",
  });
  expect((toggle as HTMLInputElement).checked).toBe(false);
  fireEvent.click(toggle);
  fireEvent.click(await screen.findByRole("button", { name: "Save changes" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ keep_history: true }),
    }),
  );
});
it("opens the History page, which links to Settings while history is off", async () => {
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "History" }));
  expect(await screen.findByText(/History is off/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Open Settings" }));
  expect(
    await screen.findByRole("checkbox", { name: "Keep history of results" }),
  ).toBeTruthy();
});
it("opens the Logs page from the top bar", async () => {
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Logs" }));
  expect(
    await screen.findByText(/never contain audio, transcripts, result text/),
  ).toBeTruthy();
  expect(ipc).toHaveBeenCalledWith("logs_page", expect.anything());
});
it("keeps timing off the Dictation page and links its status line to the Logs page", async () => {
  view.result = {
    ...suggestionResult(false),
    suggestions: [],
    status: "Copied; paste requested",
  };
  render(<App />);
  const note = await screen.findByText(/Copied; paste requested/);
  expect(note.textContent).not.toMatch(/Stop to text|\d+ ms/);
  fireEvent.click(
    screen.getByRole("button", { name: "Timing and details in Logs" }),
  );
  expect(
    await screen.findByText(/never contain audio, transcripts, result text/),
  ).toBeTruthy();
});
it("links an error the app reported to the Logs page", async () => {
  view.error = "Global hotkey unavailable; use the recording controls";
  render(<App />);
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("Global hotkey unavailable");
  fireEvent.click(within(alert).getByRole("button", { name: "View logs" }));
  expect(
    await screen.findByText(/never contain audio, transcripts, result text/),
  ).toBeTruthy();
});
it("logs a failed command and links its error banner to the Logs page", async () => {
  view.microphone = "granted";
  install("tiny.en", "silero-vad");
  const base = ipc.getMockImplementation()!;
  ipc.mockImplementation(
    async (name: string, args?: Record<string, unknown>) => {
      if (name === "start")
        throw "Enable Conch microphone access in Setup before recording";
      return base(name, args);
    },
  );
  render(<App />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Start recording" }),
  );
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("Enable Conch microphone access");
  expect(ipc).toHaveBeenCalledWith("logs_note", {
    message: "Enable Conch microphone access in Setup before recording",
  });
  fireEvent.click(within(alert).getByRole("button", { name: "View logs" }));
  expect(
    await screen.findByText(/never contain audio, transcripts, result text/),
  ).toBeTruthy();
});
it("logs a failed refresh once while it keeps failing, and again after it recovers", async () => {
  let broken = false;
  const base = ipc.getMockImplementation()!;
  ipc.mockImplementation(
    async (name: string, args?: Record<string, unknown>) => {
      if (name === "view" && broken) throw "Cannot read indexes";
      return base(name, args);
    },
  );
  render(<App />);
  await screen.findByRole("button", { name: "Settings" });
  const notes = () =>
    ipc.mock.calls.filter(
      (c) => c[0] === "logs_note" && c[1].message === "Cannot read indexes",
    ).length;
  broken = true;
  await screen.findByRole("alert");
  await new Promise((resolve) => setTimeout(resolve, 1800));
  expect(notes()).toBe(1);
  broken = false;
  await new Promise((resolve) => setTimeout(resolve, 900));
  broken = true;
  await waitFor(() => expect(notes()).toBe(2), { timeout: 3000 });
});
it("shows the installed version and checks for updates on request", async () => {
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
  expect(await screen.findByText("Conch 0.2.2")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("check_update", undefined),
  );
});
it("offers an available update and installs it when idle", async () => {
  view.update = { state: "available", detail: "0.3.0" };
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
  expect(await screen.findByText("Version 0.3.0 is available.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Install 0.3.0" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("install_update", undefined),
  );
});
it("keeps the install button disabled while a recording is in progress", async () => {
  view.update = { state: "available", detail: "0.3.0" };
  view.phase = "recording";
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
  const install = await screen.findByRole("button", { name: "Install 0.3.0" });
  expect((install as HTMLButtonElement).disabled).toBe(true);
});
it("reports a failed update check", async () => {
  view.update = { state: "failed", detail: "Cannot reach the update server" };
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
  expect(
    await screen.findByText("Cannot reach the update server"),
  ).toBeTruthy();
});

it("shows only an animation and the timer while recording", () => {
  render(
    <IndicatorView
      state={{
        visible: true,
        mode: "recording",
        label: "Listening",
        hint: "Release",
        level: 0.5,
        seconds: 7,
      }}
    />,
  );
  const status = screen.getByRole("status");
  expect(status.getAttribute("data-mode")).toBe("recording");
  expect(status.querySelectorAll(".audio-bars span")).toHaveLength(5);
  expect(status.querySelector(".spiral")).toBeNull();
  expect(status.textContent).toContain("0:07");
  expect(screen.queryByText("Release")).toBeNull();
});
it("swaps the bars for the drawing shell while processing", () => {
  render(
    <IndicatorView
      state={{
        visible: true,
        mode: "processing",
        label: "Preparing text",
        hint: "",
        level: 0,
        seconds: 9,
      }}
    />,
  );
  const status = screen.getByRole("status");
  expect(status.getAttribute("data-mode")).toBe("processing");
  expect(status.querySelector(".audio-bars")).toBeNull();
  expect(status.querySelector(".spiral")).not.toBeNull();
  expect(status.textContent).toContain("0:09");
});
it("shows the message of a delivery notice", () => {
  render(
    <IndicatorView
      state={{
        visible: true,
        mode: "notice",
        label: "Copied to clipboard",
        hint: "",
        level: 0,
        seconds: 0,
      }}
    />,
  );
  expect(screen.getByText("Copied to clipboard")).toBeTruthy();
  expect(screen.getByRole("status").querySelector(".audio-bars")).toBeNull();
});
it("navigates between pages from the top bar and keeps one save bar", async () => {
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Mappings" }));
  expect(screen.getByText("No mappings yet")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Save mappings" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Add mapping" }));
  expect(screen.getByText("Unsaved changes")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
  expect(screen.queryByText("Unsaved changes")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Repositories" }));
  expect(screen.getByText("No repositories yet")).toBeTruthy();
});
it("reads Ready when you are on the idle dictation page", async () => {
  view.microphone = "granted";
  install("tiny.en", "silero-vad");
  render(<App />);
  expect(await screen.findByText("Ready when you are")).toBeTruthy();
  expect(
    (
      screen.getByRole("button", {
        name: "Start recording",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(false);
});
it("tells the user about the 180 second recording limit before they record", async () => {
  view.microphone = "granted";
  install("tiny.en", "silero-vad");
  render(<App />);
  expect(await screen.findByText(/stop after 180 seconds/i)).toBeTruthy();
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
it("lists a mapping saved by Accept and always replace without leaving a change to save", async () => {
  view.result = suggestionResult(false);
  ipc.mockImplementation(async (name: string) => {
    if (name === "view") return structuredClone(view);
    if (name === "accept_always")
      (view.settings.mappings as unknown[]).push({
        phrase: "max group",
        replacement: "maxGroups",
        repository: "r",
        enabled: true,
      });
    return;
  });
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
  fireEvent.click(screen.getByRole("button", { name: "Mappings" }));
  const phrase = await screen.findByLabelText("Spoken phrase");
  expect((phrase as HTMLInputElement).value).toBe("max group");
  expect(screen.queryByRole("button", { name: "Save changes" })).toBeNull();
});
it("does not offer to remember a mapping conflict", async () => {
  view.result = suggestionResult(true);
  render(<App />);
  await screen.findByRole("button", { name: "Accept" });
  expect(
    screen.queryByRole("button", { name: "Accept and always replace" }),
  ).toBeNull();
});

const models = () => screen.queryByRole("heading", { name: "Local models" });
it("has no separate list of local models; every download sits beside its menu", async () => {
  install("tiny.en", "silero-vad");
  render(<App />);
  await openSettings();
  await screen.findByRole("combobox", { name: "Recognition model" });
  expect(models()).toBeNull();
  expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
  expect(screen.getByText("Licenses and acknowledgments")).toBeTruthy();
  expect(screen.getByText(/core dictation works offline/i)).toBeTruthy();
});
it("builds the recognition model menu from the models the view returns", async () => {
  view.models[0].label = "Tiny (from the catalog)";
  view.models[0].note = "Quick";
  view.models.splice(3, 0, {
    id: "extra.en",
    label: "Extra (English)",
    note: "Experimental",
    size: 2_000_000_000,
    engine: "whisper",
    languages: ["en"],
    ready: false,
  });
  render(<App />);
  await openSettings();
  const menu = (await screen.findByRole("combobox", {
    name: "Recognition model",
  })) as HTMLSelectElement;
  expect(Array.from(menu.options).map((option) => option.textContent)).toEqual([
    "Tiny (from the catalog) - 77.7 MB - Quick",
    "Base (English) - 148.0 MB - Balanced",
    "Small (English) - 487.6 MB - Most accurate",
    "Extra (English) - 2000.0 MB - Experimental",
    "Tiny (multilingual) - 77.7 MB - Fastest",
    "Base (multilingual) - 148.0 MB - Balanced",
    "Small (multilingual) - 487.6 MB - Accurate",
    "Large Turbo (multilingual) - 574.0 MB - Most accurate",
  ]);
  fireEvent.change(menu, { target: { value: "extra.en" } });
  expect(
    screen.getByRole("button", { name: "Download Extra (English), 2000.0 MB" }),
  ).toBeTruthy();
});
const menuOptions = (name: string) =>
  Array.from(
    (screen.getByRole("combobox", { name }) as HTMLSelectElement).options,
  );
it("lists Auto-detect first, then English, then every other language by name", async () => {
  render(<App />);
  await openSettings();
  const menu = (await screen.findByRole("combobox", {
    name: "Language",
  })) as HTMLSelectElement;
  expect(menu.value).toBe("en");
  expect(menuOptions("Language").map((option) => option.textContent)).toEqual([
    "Auto-detect",
    "English",
    "Cantonese",
    "Chinese",
    "French",
    "German",
  ]);
  expect(menuOptions("Language").map((option) => option.value)).toEqual([
    "auto",
    "en",
    "yue",
    "zh",
    "fr",
    "de",
  ]);
  expect(menuOptions("Language").every((option) => !option.disabled)).toBe(
    true,
  );
});
it("offers Auto-detect only while a multilingual model exists", async () => {
  view.models = view.models.filter(
    (model) => model.engine !== "whisper" || model.languages.length === 1,
  );
  render(<App />);
  await openSettings();
  await screen.findByRole("combobox", { name: "Language" });
  expect(menuOptions("Language").map((option) => option.value)).not.toContain(
    "auto",
  );
});
it("switches an English-only model to Small when Auto-detect is chosen and keeps a multilingual one", async () => {
  render(<App />);
  await openSettings();
  await chooseLanguage("auto");
  expect(
    menuOptions("Recognition model").map((option) => option.value),
  ).toEqual(["tiny", "base", "small", "large-v3-turbo"]);
  expect(
    (
      screen.getByRole("combobox", {
        name: "Recognition model",
      }) as HTMLSelectElement
    ).value,
  ).toBe("small");
  fireEvent.change(
    screen.getByRole("combobox", { name: "Recognition model" }),
    {
      target: { value: "base" },
    },
  );
  await chooseLanguage("fr");
  await chooseLanguage("auto");
  expect(
    (
      screen.getByRole("combobox", {
        name: "Recognition model",
      }) as HTMLSelectElement
    ).value,
  ).toBe("base");
  const saved = await savedSettings();
  expect(saved.language).toBe("auto");
  expect(saved.recognizer).toBe("base");
});
it("chooses Auto-detect on the setup card and saves it with a multilingual model at once", async () => {
  render(<App />);
  fireEvent.change(await screen.findByRole("combobox", { name: "Language" }), {
    target: { value: "auto" },
  });
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({
        language: "auto",
        recognizer: "small",
      }),
    }),
  );
});
it("shows only the models that support the chosen language and switches an unsupported model to Small", async () => {
  render(<App />);
  await openSettings();
  fireEvent.change(await screen.findByRole("combobox", { name: "Language" }), {
    target: { value: "fr" },
  });
  expect(
    menuOptions("Recognition model").map((option) => option.value),
  ).toEqual(["tiny", "base", "small", "large-v3-turbo"]);
  expect(
    (
      screen.getByRole("combobox", {
        name: "Recognition model",
      }) as HTMLSelectElement
    ).value,
  ).toBe("small");
});
const tokens = () =>
  screen.getByLabelText("Hesitation tokens") as HTMLInputElement;
const chooseLanguage = async (code: string) =>
  fireEvent.change(await screen.findByRole("combobox", { name: "Language" }), {
    target: { value: code },
  });
it("edits the hesitation tokens of the selected language and keeps the other lists", async () => {
  render(<App />);
  await openSettings();
  await screen.findByLabelText("Hesitation tokens");
  expect(tokens().value).toBe("um,uh");
  expect(tokens().disabled).toBe(false);
  await chooseLanguage("fr");
  expect(tokens().value).toBe("");
  fireEvent.change(tokens(), { target: { value: "euh,ben" } });
  await chooseLanguage("en");
  expect(tokens().value).toBe("um,uh");
  await chooseLanguage("fr");
  expect(tokens().value).toBe("euh,ben");
  const saved = await savedSettings();
  expect(saved.cleanup.fillers).toEqual({
    en: ["um", "uh"],
    fr: ["euh", "ben"],
  });
});
it("shows Auto-detect in the Language menu when that is the saved language", async () => {
  view.settings.language = "auto";
  view.settings.recognizer = "small";
  render(<App />);
  await openSettings();
  const menu = (await screen.findByRole("combobox", {
    name: "Language",
  })) as HTMLSelectElement;
  expect(menu.value).toBe("auto");
  expect(menu.selectedOptions[0].textContent).toBe("Auto-detect");
  expect(Array.from(menu.options).map((option) => option.value)).toEqual([
    "auto",
    "en",
    "yue",
    "zh",
    "fr",
    "de",
  ]);
  fireEvent.change(menu, { target: { value: "fr" } });
  expect(menu.value).toBe("fr");
  expect(Array.from(menu.options).map((option) => option.value)).toContain(
    "auto",
  );
});
it("warns that Auto-detect is less reliable for short recordings and says the first chunk sets the language", async () => {
  render(<App />);
  await openSettings();
  await screen.findByRole("combobox", { name: "Language" });
  expect(screen.queryByText(/auto-detect is less reliable/i)).toBeNull();
  await chooseLanguage("auto");
  expect(
    screen.getByText(
      /auto-detect is less reliable for recordings under a few seconds.*first chunk/i,
    ),
  ).toBeTruthy();
  await chooseLanguage("fr");
  expect(screen.queryByText(/auto-detect is less reliable/i)).toBeNull();
});
it("disables the Hesitation tokens row for Auto-detect and says why", async () => {
  view.settings.language = "auto";
  view.settings.recognizer = "small";
  render(<App />);
  await openSettings();
  await screen.findByLabelText("Hesitation tokens");
  expect(tokens().disabled).toBe(true);
  expect(tokens().value).toBe("");
  expect(screen.getByText(/auto-detect removes none/i)).toBeTruthy();
});
it("says that managed S1-mini cleanup and spelled-letter matching are English only", async () => {
  render(<App />);
  await openSettings();
  fireEvent.change(
    await screen.findByRole("combobox", { name: "Enhanced cleanup" }),
    { target: { value: "managed" } },
  );
  expect(screen.queryByText(/supports English only/i)).toBeNull();
  expect(screen.queryByText(/spelled-letter matching/i)).toBeNull();
  await chooseLanguage("fr");
  expect(
    screen.getByText(/managed s1-mini supports english only.*basic cleanup/i),
  ).toBeTruthy();
  expect(
    screen.getByText(/spelled-letter matching.*english only/i),
  ).toBeTruthy();
  await chooseLanguage("en");
  expect(screen.queryByText(/supports English only/i)).toBeNull();
  expect(screen.queryByText(/spelled-letter matching/i)).toBeNull();
});
it("explains the Basic cleanup fallback for Auto-detect when managed cleanup is chosen", async () => {
  view.settings.language = "auto";
  view.settings.recognizer = "small";
  view.settings.cleanup.enhanced = "managed";
  render(<App />);
  await openSettings();
  expect(
    await screen.findByText(/managed s1-mini supports english only/i),
  ).toBeTruthy();
});
it("switches to Large Turbo for Cantonese, the only model that has it", async () => {
  view.settings.recognizer = "small";
  render(<App />);
  await openSettings();
  fireEvent.change(await screen.findByRole("combobox", { name: "Language" }), {
    target: { value: "yue" },
  });
  expect(
    menuOptions("Recognition model").map((option) => option.value),
  ).toEqual(["large-v3-turbo"]);
  expect(
    (
      screen.getByRole("combobox", {
        name: "Recognition model",
      }) as HTMLSelectElement
    ).value,
  ).toBe("large-v3-turbo");
});
it("keeps a valid language and model pair when the setup card saves a model while Settings holds an unsaved language", async () => {
  render(<App />);
  await openSettings();
  fireEvent.change(await screen.findByRole("combobox", { name: "Language" }), {
    target: { value: "fr" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Dictation" }));
  fireEvent.change(
    await screen.findByRole("combobox", { name: "Recognition model" }),
    { target: { value: "base.en" } },
  );
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({
        language: "en",
        recognizer: "base.en",
      }),
    }),
  );
  await openSettings();
  expect(
    (screen.getByRole("combobox", { name: "Language" }) as HTMLSelectElement)
      .value,
  ).toBe("fr");
  const model = screen.getByRole("combobox", {
    name: "Recognition model",
  }) as HTMLSelectElement;
  expect(Array.from(model.options).map((option) => option.value)).toContain(
    model.value,
  );
  fireEvent.click(await screen.findByRole("button", { name: "Save changes" }));
  const saves = () =>
    ipc.mock.calls.filter(([name]) => name === "save_settings");
  await waitFor(() => expect(saves()).toHaveLength(2));
  const saved = saves()[1][1].settings as typeof view.settings;
  expect(saved.language).toBe("fr");
  expect(saved.recognizer).toBe("small");
});
it("keeps the model when it supports the new language and saves the language with it", async () => {
  view.settings.language = "fr";
  view.settings.recognizer = "base";
  render(<App />);
  await openSettings();
  fireEvent.change(await screen.findByRole("combobox", { name: "Language" }), {
    target: { value: "de" },
  });
  expect(
    (
      screen.getByRole("combobox", {
        name: "Recognition model",
      }) as HTMLSelectElement
    ).value,
  ).toBe("base");
  const saved = await savedSettings();
  expect(saved.language).toBe("de");
  expect(saved.recognizer).toBe("base");
});
it("switches to Small (English) when the language returns to English and the model lacks it", async () => {
  view.models.find((model) => model.id === "tiny.en")!.languages = ["fr"];
  render(<App />);
  await openSettings();
  fireEvent.change(await screen.findByRole("combobox", { name: "Language" }), {
    target: { value: "fr" },
  });
  fireEvent.change(screen.getByRole("combobox", { name: "Language" }), {
    target: { value: "en" },
  });
  expect(
    (
      screen.getByRole("combobox", {
        name: "Recognition model",
      }) as HTMLSelectElement
    ).value,
  ).toBe("small.en");
});
it("chooses the language on the setup card and saves it with its model at once", async () => {
  render(<App />);
  fireEvent.change(await screen.findByRole("combobox", { name: "Language" }), {
    target: { value: "fr" },
  });
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({
        language: "fr",
        recognizer: "small",
      }),
    }),
  );
  expect(
    await screen.findByRole("button", {
      name: "Download Small (multilingual), 487.6 MB",
    }),
  ).toBeTruthy();
});
it("marks installed recognition models in the menu and confirms the selected one is installed", async () => {
  install("tiny.en", "base.en", "silero-vad");
  render(<App />);
  await openSettings();
  const menu = (await screen.findByRole("combobox", {
    name: "Recognition model",
  })) as HTMLSelectElement;
  expect(Array.from(menu.options).map((option) => option.textContent)).toEqual([
    "Tiny (English) - Installed - Fastest",
    "Base (English) - Installed - Balanced",
    "Small (English) - 487.6 MB - Most accurate",
    "Tiny (multilingual) - 77.7 MB - Fastest",
    "Base (multilingual) - 148.0 MB - Balanced",
    "Small (multilingual) - 487.6 MB - Accurate",
    "Large Turbo (multilingual) - 574.0 MB - Most accurate",
  ]);
  expect(
    screen.getByText(
      "Tiny (English) is installed. Speech stays on this device.",
    ),
  ).toBeTruthy();
  fireEvent.change(menu, { target: { value: "small.en" } });
  expect(screen.queryByText(/Small \(English\) is installed/)).toBeNull();
  expect(
    screen.getByRole("button", { name: "Download Small (English), 487.6 MB" }),
  ).toBeTruthy();
});
it("shows download progress next to the model menu and then confirms the install", async () => {
  install("tiny.en", "silero-vad");
  let release = () => {};
  holdDownload = new Promise((resolve) => (release = resolve));
  render(<App />);
  await openSettings();
  fireEvent.change(
    await screen.findByRole("combobox", { name: "Recognition model" }),
    { target: { value: "small.en" } },
  );
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Download Small (English), 487.6 MB",
    }),
  );
  const bar = await screen.findByRole("progressbar", {
    name: "Download progress",
  });
  expect(bar.hasAttribute("aria-valuenow")).toBe(false);
  await waitFor(() => expect(events.progress).not.toBeNull());
  act(() => events.progress?.({ payload: [50, 100] }));
  expect(
    (await screen.findByRole("progressbar")).getAttribute("aria-valuenow"),
  ).toBe("50");
  expect(screen.getByText("Downloading Small (English), 50%")).toBeTruthy();
  expect(
    (
      screen.getByRole("combobox", {
        name: "Recognition model",
      }) as HTMLSelectElement
    ).disabled,
  ).toBe(true);
  release();
  expect(
    await screen.findByText(
      "Small (English) is installed. Speech stays on this device.",
    ),
  ).toBeTruthy();
  expect(screen.queryByRole("progressbar")).toBeNull();
});
it("shows S1-mini progress and its installed state under Enhanced cleanup", async () => {
  holdDownload = new Promise(() => {});
  render(<App />);
  await openSettings();
  fireEvent.change(
    await screen.findByRole("combobox", { name: "Enhanced cleanup" }),
    { target: { value: "managed" } },
  );
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Download S1-mini by Superwhisper, 484.2 MB",
    }),
  );
  expect(
    await screen.findByText("Downloading S1-mini by Superwhisper"),
  ).toBeTruthy();
});
it("confirms S1-mini is installed once Managed cleanup is chosen", async () => {
  install("s1-mini");
  render(<App />);
  await openSettings();
  fireEvent.change(
    await screen.findByRole("combobox", { name: "Enhanced cleanup" }),
    { target: { value: "managed" } },
  );
  expect(
    await screen.findByText("S1-mini by Superwhisper is installed."),
  ).toBeTruthy();
});
it("tells a copy with the recognizer but no voice detection that one small download is missing", async () => {
  install("tiny.en");
  render(<App />);
  await openSettings();
  expect(
    await screen.findByRole("button", {
      name: "Download Voice detection (Silero), 0.9 MB",
    }),
  ).toBeTruthy();
  expect(screen.queryByText(/Tiny \(English\) is installed/)).toBeNull();
});
