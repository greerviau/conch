import { afterEach, beforeEach, it, expect, vi } from "vitest";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
  within,
} from "@testing-library/react";
import { Logs } from "./Logs";
import type { LogEntry } from "./types";
const ipc = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc }));
const noon = new Date();
noon.setHours(12, 0, 0, 0);
const TODAY = noon.getTime();
const YESTERDAY = TODAY - 24 * 3600 * 1000;
type Args = {
  level: string;
  source: string;
  query: string;
  offset: number;
  limit: number;
};
let entries: LogEntry[] = [];
const entry = (
  n: number,
  level: LogEntry["level"],
  source: string,
  message: string,
  detail?: string,
  day = TODAY,
): LogEntry => ({ time: day + n * 1000, level, source, message, detail });
/** The backend's filters, newest first. */
const respond = async (name: string, args?: Args) => {
  if (name === "logs_page") {
    const q = args!.query.toLowerCase();
    const match = entries.filter(
      (e) =>
        (args!.level === "all" ||
          (args!.level === "warn"
            ? e.level !== "info"
            : e.level === "error")) &&
        (args!.source === "all" || e.source === args!.source) &&
        (!q ||
          `${e.message} ${e.detail ?? ""} ${e.source}`
            .toLowerCase()
            .includes(q)),
    );
    return {
      items: match.slice(args!.offset, args!.offset + args!.limit),
      total: match.length,
      all: entries.length,
      sources: [...new Set(entries.map((e) => e.source))].sort(),
      bytes: 1_468_006,
    };
  }
  if (name === "logs_clear") entries = [];
};
beforeEach(() => {
  ipc.mockReset();
  entries = [
    entry(
      3,
      "info",
      "dictation",
      "Stop to text 412 ms",
      "capture 12 ms\nrecognition 301 ms",
    ),
    entry(2, "error", "cleanup", "Enhanced cleanup failed: timed out"),
    entry(1, "warn", "delivery", "Copied only: no focused text field"),
    entry(9, "info", "app", "Conch 0.3.0 started", undefined, YESTERDAY),
  ];
  ipc.mockImplementation(respond);
});
afterEach(cleanup);
const open = (revision = 1) => render(<Logs revision={revision} />);
const time = (ms: number) =>
  new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
it("lists the entries newest first under the day they happened with their time, level, and source", async () => {
  open();
  expect(await screen.findByText("Stop to text 412 ms")).toBeTruthy();
  expect(ipc).toHaveBeenCalledWith("logs_page", {
    level: "all",
    source: "all",
    query: "",
    offset: 0,
    limit: 50,
  });
  const text = document.body.textContent ?? "";
  expect(text.indexOf("Today")).toBeLessThan(text.indexOf("Stop to text"));
  expect(text.indexOf("Stop to text")).toBeLessThan(
    text.indexOf("Enhanced cleanup failed"),
  );
  expect(text.indexOf("Copied only")).toBeLessThan(text.indexOf("Yesterday"));
  expect(text.indexOf("Yesterday")).toBeLessThan(text.indexOf("Conch 0.3.0"));
  const row = screen
    .getByText("Enhanced cleanup failed: timed out")
    .closest(".log")!;
  expect(within(row as HTMLElement).getByText("Error")).toBeTruthy();
  expect(within(row as HTMLElement).getByText("cleanup")).toBeTruthy();
  expect(within(row as HTMLElement).getByText(time(TODAY + 2000))).toBeTruthy();
});
it("shows an entry's detail only when it has one, behind a Details toggle", async () => {
  open();
  const row = (await screen.findByText("Stop to text 412 ms")).closest(".log")!;
  const details = row as HTMLDetailsElement;
  expect(details.tagName).toBe("DETAILS");
  expect(details.open).toBe(false);
  fireEvent.click(details.querySelector("summary")!);
  expect(details.open).toBe(true);
  expect(details.querySelector("pre")!.textContent).toBe(
    "capture 12 ms\nrecognition 301 ms",
  );
  const plain = screen
    .getByText("Copied only: no focused text field")
    .closest(".log")!;
  expect(plain.tagName).toBe("DIV");
});
it("narrows to warnings or errors and to one source", async () => {
  open();
  await screen.findByText("Stop to text 412 ms");
  fireEvent.click(screen.getByRole("button", { name: "Warnings" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenLastCalledWith(
      "logs_page",
      expect.objectContaining({ level: "warn", offset: 0 }),
    ),
  );
  expect(
    await screen.findByText("Copied only: no focused text field"),
  ).toBeTruthy();
  expect(screen.queryByText("Stop to text 412 ms")).toBeNull();
  expect(
    screen
      .getByRole("button", { name: "Warnings" })
      .getAttribute("aria-pressed"),
  ).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: "Errors" }));
  await waitFor(() =>
    expect(screen.queryByText("Copied only: no focused text field")).toBeNull(),
  );
  fireEvent.click(screen.getByRole("button", { name: "All" }));
  await screen.findByText("Stop to text 412 ms");
  const source = screen.getByLabelText("Source") as HTMLSelectElement;
  expect([...source.options].map((o) => o.value)).toEqual([
    "all",
    "app",
    "cleanup",
    "delivery",
    "dictation",
  ]);
  fireEvent.change(source, { target: { value: "cleanup" } });
  await waitFor(() =>
    expect(screen.queryByText("Stop to text 412 ms")).toBeNull(),
  );
  expect(screen.getByText("Enhanced cleanup failed: timed out")).toBeTruthy();
});
it("searches after a pause in typing and marks the match", async () => {
  open();
  await screen.findByText("Stop to text 412 ms");
  fireEvent.change(screen.getByLabelText("Search logs"), {
    target: { value: "cleanup" },
  });
  await waitFor(() =>
    expect(ipc).toHaveBeenLastCalledWith(
      "logs_page",
      expect.objectContaining({ query: "cleanup", offset: 0 }),
    ),
  );
  await waitFor(() =>
    expect(screen.queryByText("Stop to text 412 ms")).toBeNull(),
  );
  expect(document.querySelector("mark")!.textContent).toBe("cleanup");
  fireEvent.change(screen.getByLabelText("Search logs"), {
    target: { value: "zzz" },
  });
  expect(await screen.findByText("No matches")).toBeTruthy();
});
it("pages through a long log 50 entries at a time", async () => {
  entries = Array.from({ length: 120 }, (_, i) =>
    entry(i, "info", "app", `entry ${i}`),
  ).reverse();
  open();
  expect(await screen.findByText(/^1-50 of 120 entries/)).toBeTruthy();
  expect(screen.getByText(/1\.5 MB/)).toBeTruthy();
  expect(
    (screen.getByRole("button", { name: "Previous" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(await screen.findByText(/^51-100 of 120 entries/)).toBeTruthy();
  expect(ipc).toHaveBeenLastCalledWith(
    "logs_page",
    expect.objectContaining({ offset: 50 }),
  );
});
it("reloads when the log changes", async () => {
  const view = open(1);
  await screen.findByText("Stop to text 412 ms");
  entries = [entry(5, "info", "app", "A new entry"), ...entries];
  view.rerender(<Logs revision={2} />);
  expect(await screen.findByText("A new entry")).toBeTruthy();
});
it("copies the shown entries with their detail as text", async () => {
  open();
  await screen.findByText("Stop to text 412 ms");
  fireEvent.click(screen.getByRole("button", { name: "Copy shown entries" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("logs_copy", expect.anything()),
  );
  const text = ipc.mock.calls.find((c) => c[0] === "logs_copy")![1]
    .text as string;
  expect(text).toContain(
    `${new Date(TODAY + 3000).toISOString()} INFO dictation Stop to text 412 ms\n  capture 12 ms\n  recognition 301 ms\n`,
  );
  expect(text).toContain(" ERROR cleanup Enhanced cleanup failed: timed out\n");
  expect(await screen.findByText("Copied 4 entries")).toBeTruthy();
});
it("shows the log file in the file manager", async () => {
  open();
  await screen.findByText("Stop to text 412 ms");
  fireEvent.click(screen.getByRole("button", { name: "Show log file" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("logs_reveal", undefined),
  );
});
it("clears every entry only after a confirmation", async () => {
  open();
  await screen.findByText("Stop to text 412 ms");
  fireEvent.click(screen.getByRole("button", { name: "Clear logs" }));
  expect(
    screen.getByText(
      "Delete all log entries, including those hidden by the filters?",
    ),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(ipc).not.toHaveBeenCalledWith("logs_clear", undefined);
  fireEvent.click(screen.getByRole("button", { name: "Clear logs" }));
  fireEvent.click(screen.getByRole("button", { name: "Delete all" }));
  expect(await screen.findByText(/Nothing logged yet/)).toBeTruthy();
  expect(ipc).toHaveBeenCalledWith("logs_clear", undefined);
  expect(screen.queryByLabelText("Search logs")).toBeNull();
});
it("says nothing is logged yet and offers no filters for an empty log", async () => {
  entries = [];
  open();
  expect(await screen.findByText(/Nothing logged yet/)).toBeTruthy();
  expect(screen.queryByLabelText("Search logs")).toBeNull();
  expect(screen.queryByRole("button", { name: "Clear logs" })).toBeNull();
});
it("reports a failure to read the log", async () => {
  ipc.mockImplementation(async () => {
    throw "The log is unavailable";
  });
  open();
  expect((await screen.findByRole("alert")).textContent).toBe(
    "The log is unavailable",
  );
});
it("states that logs never hold transcripts", async () => {
  open();
  expect(
    await screen.findByText(/never contain audio, transcripts, result text/),
  ).toBeTruthy();
});
