import { afterEach, beforeEach, it, expect, vi } from "vitest";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
  act,
} from "@testing-library/react";
import App from "./App";
import { IndicatorView } from "./RecordingIndicator";
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
    auto_paste: true,
    microphone: "",
    show_overlay: true,
    check_updates: true,
    cleanup: {
      fillers: ["um", "uh"],
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
  models: [
    ["tiny.en", false],
    ["base.en", false],
    ["small.en", false],
    ["silero-vad", false],
    ["s1-mini", false],
  ] as [string, boolean][],
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
    if (ids.includes(model[0])) model[1] = true;
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
        cleanup: expect.objectContaining({ fillers: ["um", "uh"] }),
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
