import { afterEach, describe, it, expect, vi } from "vitest";
import {
  render,
  screen,
  cleanup,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { Setup, ShortcutRecorder } from "./Setup";
import { LANGUAGES, catalog } from "./testModels";
import type { ComponentProps } from "react";
type Defaults = "models" | "languages" | "language" | "recognizer";
type Props = Omit<ComponentProps<typeof Setup>, Defaults> &
  Partial<Pick<ComponentProps<typeof Setup>, Defaults>>;
const Subject = (props: Props) => (
  <Setup
    models={catalog()}
    languages={LANGUAGES}
    language="en"
    recognizer="small.en"
    {...props}
  />
);
afterEach(cleanup);
describe("first-run setup", () => {
  it("offers the required download where dictation starts", () => {
    const download = vi.fn();
    render(<Subject installed={false} onDownload={download} />);
    fireEvent.click(
      screen.getByRole("button", {
        name: "Download Small (English), 487.6 MB",
      }),
    );
    expect(download).toHaveBeenCalledOnce();
  });
  it("does not ask users to install optional cleanup to dictate", () => {
    render(<Subject installed onDownload={vi.fn()} />);
    expect(screen.queryByRole("button", { name: /^download/i })).toBeNull();
  });
});
describe("recognition language", () => {
  const menu = (name: string) =>
    screen.getByRole("combobox", { name }) as HTMLSelectElement;
  it("names the step for speech recognition instead of English", () => {
    render(<Subject installed={false} onDownload={vi.fn()} />);
    expect(
      screen.getByRole("heading", { name: "Speech recognition" }),
    ).toBeTruthy();
    expect(screen.queryByText("English recognition")).toBeNull();
  });
  it("offers a Language menu, English first, and reports the choice", () => {
    const choose = vi.fn();
    render(
      <Subject installed={false} onLanguage={choose} onDownload={vi.fn()} />,
    );
    expect(menu("Language").value).toBe("en");
    expect(
      Array.from(menu("Language").options).map((option) => option.textContent),
    ).toEqual([
      "Auto-detect",
      "English",
      "Cantonese",
      "Chinese",
      "French",
      "German",
    ]);
    fireEvent.change(menu("Language"), { target: { value: "fr" } });
    expect(choose).toHaveBeenCalledWith("fr");
  });
  it("reports Auto-detect like any other choice", () => {
    const choose = vi.fn();
    render(
      <Subject installed={false} onLanguage={choose} onDownload={vi.fn()} />,
    );
    fireEvent.change(menu("Language"), { target: { value: "auto" } });
    expect(choose).toHaveBeenCalledWith("auto");
  });
  it("offers no Auto-detect without a multilingual model", () => {
    const english = catalog().filter(
      (model) => model.engine !== "whisper" || model.languages.length === 1,
    );
    render(<Subject installed={false} models={english} onDownload={vi.fn()} />);
    expect(
      Array.from(menu("Language").options).map((option) => option.value),
    ).not.toContain("auto");
  });
  it("shows Auto-detect when that is the saved language", () => {
    render(
      <Subject
        installed={false}
        language="auto"
        recognizer="small"
        onDownload={vi.fn()}
      />,
    );
    expect(menu("Language").value).toBe("auto");
    expect(menu("Language").selectedOptions[0].textContent).toBe("Auto-detect");
  });
  it("lists only the models that support the saved language", () => {
    render(
      <Subject
        installed={false}
        language="fr"
        recognizer="small"
        onDownload={vi.fn()}
      />,
    );
    expect(
      Array.from(menu("Recognition model").options).map(
        (option) => option.value,
      ),
    ).toEqual(["tiny", "base", "small", "large-v3-turbo"]);
    expect(
      screen.getByRole("button", {
        name: "Download Small (multilingual), 487.6 MB",
      }),
    ).toBeTruthy();
  });
});
describe("recognition model step", () => {
  const options = () =>
    Array.from(
      (
        screen.getByRole("combobox", {
          name: "Recognition model",
        }) as HTMLSelectElement
      ).options,
    );
  it("lets the user pick any recognition model and names it on the download button", () => {
    const choose = vi.fn();
    const download = vi.fn();
    render(
      <Subject
        installed={false}
        recognizer="small.en"
        onRecognizer={choose}
        onDownload={download}
      />,
    );
    const menu = screen.getByRole("combobox", {
      name: "Recognition model",
    }) as HTMLSelectElement;
    expect(menu.value).toBe("small.en");
    expect(options().map((option) => option.value)).toEqual([
      "tiny.en",
      "base.en",
      "small.en",
      "tiny",
      "base",
      "small",
      "large-v3-turbo",
    ]);
    fireEvent.click(
      screen.getByRole("button", {
        name: "Download Small (English), 487.6 MB",
      }),
    );
    expect(download).toHaveBeenCalledOnce();
    fireEvent.change(menu, { target: { value: "base.en" } });
    expect(choose).toHaveBeenCalledWith("base.en");
  });
  it("shows Installed in the menu instead of a size for a model that is on disk", () => {
    render(
      <Subject
        installed={false}
        recognizer="small.en"
        ready={(id) => id === "tiny.en"}
        onDownload={vi.fn()}
      />,
    );
    expect(options().map((option) => option.textContent)).toEqual([
      "Tiny (English) - Installed - Fastest",
      "Base (English) - 148.0 MB - Balanced",
      "Small (English) - 487.6 MB - Most accurate",
      "Tiny (multilingual) - 77.7 MB - Fastest",
      "Base (multilingual) - 148.0 MB - Balanced",
      "Small (multilingual) - 487.6 MB - Accurate",
      "Large Turbo (multilingual) - 574.0 MB - Most accurate",
    ]);
  });
  it("lists the models it is given and sizes the voice detection note from them", () => {
    const models = catalog().map((model) =>
      model.id === "silero-vad"
        ? { ...model, size: 1_500_000 }
        : model.id === "tiny.en"
          ? { ...model, label: "Tiny (catalog)" }
          : model,
    );
    render(<Subject installed={false} models={models} onDownload={vi.fn()} />);
    expect(options().map((option) => option.textContent)).toEqual([
      "Tiny (catalog) - 77.7 MB - Fastest",
      "Base (English) - 148.0 MB - Balanced",
      "Small (English) - 487.6 MB - Most accurate",
      "Tiny (multilingual) - 77.7 MB - Fastest",
      "Base (multilingual) - 148.0 MB - Balanced",
      "Small (multilingual) - 487.6 MB - Accurate",
      "Large Turbo (multilingual) - 574.0 MB - Most accurate",
    ]);
    expect(
      screen.getByText(/also installs the 1\.5 MB voice detection/i),
    ).toBeTruthy();
  });
  it("says what else the download installs and why the models differ", () => {
    render(<Subject installed={false} onDownload={vi.fn()} />);
    expect(
      screen.getByText(/also installs the 0\.9 MB voice detection/i),
    ).toBeTruthy();
    expect(
      screen.getByText(/take longer to finish after you stop speaking/i),
    ).toBeTruthy();
  });
  it("replaces the button with a progress bar that names the model", () => {
    const { rerender } = render(
      <Subject
        installed={false}
        recognizer="small.en"
        downloading
        percent={40}
        onDownload={vi.fn()}
      />,
    );
    expect(
      screen
        .getByRole("progressbar", { name: "Download progress" })
        .getAttribute("aria-valuenow"),
    ).toBe("40");
    expect(screen.getByText("Downloading Small (English), 40%")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^download/i })).toBeNull();
    expect(
      (
        screen.getByRole("combobox", {
          name: "Recognition model",
        }) as HTMLSelectElement
      ).disabled,
    ).toBe(true);
    rerender(
      <Subject
        installed={false}
        recognizer="small.en"
        downloading
        percent={null}
        onDownload={vi.fn()}
      />,
    );
    expect(
      screen
        .getByRole("progressbar", { name: "Download progress" })
        .hasAttribute("aria-valuenow"),
    ).toBe(false);
    expect(screen.getByText(/^Downloading Small \(English\)/)).toBeTruthy();
  });
  it("names the installed model and reopens the menu from Change model", () => {
    render(<Subject installed recognizer="base.en" onDownload={vi.fn()} />);
    expect(screen.getByText(/Base \(English\) is installed/)).toBeTruthy();
    expect(screen.queryByRole("combobox")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Change model" }));
    expect(
      screen.getByRole("combobox", { name: "Recognition model" }),
    ).toBeTruthy();
  });
  it("closes the menu again once the user picks a model that is installed", () => {
    const choose = vi.fn();
    render(
      <Subject
        installed
        recognizer="tiny.en"
        ready={() => true}
        onRecognizer={choose}
        onDownload={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Change model" }));
    fireEvent.change(
      screen.getByRole("combobox", { name: "Recognition model" }),
      { target: { value: "base.en" } },
    );
    expect(choose).toHaveBeenCalledWith("base.en");
    expect(screen.queryByRole("combobox")).toBeNull();
  });
  it("asks only for the small voice detection file when the recognizer is already installed", () => {
    const download = vi.fn();
    render(
      <Subject
        installed={false}
        voiceOnly
        recognizer="tiny.en"
        onDownload={download}
      />,
    );
    expect(screen.getByText(/0\.9 MB voice detection model/)).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Download Voice detection (Silero), 0.9 MB",
      }),
    );
    expect(download).toHaveBeenCalledOnce();
  });
});
describe("paste step", () => {
  it("asks for the paste permission only while automatic paste is on", () => {
    const { rerender } = render(
      <Subject
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
      <Subject
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
    <Subject
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
    <Subject
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
