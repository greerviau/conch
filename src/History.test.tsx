import { afterEach, beforeEach, it, expect, vi } from "vitest";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
} from "@testing-library/react";
import { History } from "./History";
const ipc = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc }));
const BASE = 1_700_000_000;
type Item = { id: number; created: number; text: string; raw: string };
let items: Item[] = [];
const entry = (id: number, text: string, raw = text): Item => ({
  id,
  created: BASE + id * 60,
  text,
  raw,
});
const respond = async (
  name: string,
  args?: { query: string; offset: number; limit: number; id: number },
) => {
  if (name === "history_page") {
    const q = args!.query.toLowerCase();
    const match = items.filter((i) => i.text.toLowerCase().includes(q));
    return {
      total: match.length,
      items: match.slice(args!.offset, args!.offset + args!.limit),
    };
  }
  if (name === "history_delete") items = items.filter((i) => i.id !== args!.id);
  if (name === "history_clear") items = [];
};
beforeEach(() => {
  ipc.mockReset();
  items = [
    entry(2, "open main.rs", "um open main dot rs"),
    entry(1, "fetchUserProfile"),
  ];
  ipc.mockImplementation(respond);
});
afterEach(cleanup);
const open = (props: Partial<Parameters<typeof History>[0]> = {}) =>
  render(
    <History enabled={true} revision={2} onSettings={vi.fn()} {...props} />,
  );
it("lists the saved results newest first with the time they were delivered", async () => {
  open();
  expect(await screen.findByText("open main.rs")).toBeTruthy();
  expect(screen.getByText("fetchUserProfile")).toBeTruthy();
  expect(screen.getByText("1-2 of 2 entries")).toBeTruthy();
  expect(screen.getByText(new Date((BASE + 120) * 1000).toLocaleString()));
  expect(ipc).toHaveBeenCalledWith("history_page", {
    query: "",
    offset: 0,
    limit: 50,
  });
  const text = document.body.textContent ?? "";
  expect(text.indexOf("open main.rs")).toBeLessThan(
    text.indexOf("fetchUserProfile"),
  );
});
it("says history is off and points to Settings when nothing was saved", async () => {
  items = [];
  const onSettings = vi.fn();
  open({ enabled: false, revision: 0, onSettings });
  expect(await screen.findByText(/History is off/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Open Settings" }));
  expect(onSettings).toHaveBeenCalled();
});
it("keeps showing older entries while history is off and says new results are not saved", async () => {
  open({ enabled: false });
  expect(await screen.findByText("open main.rs")).toBeTruthy();
  expect(screen.getByText(/New results are not saved/)).toBeTruthy();
});
it("says nothing was saved yet when history is on and empty", async () => {
  items = [];
  open({ revision: 0 });
  expect(await screen.findByText("No results saved yet")).toBeTruthy();
});
it("copies the result or the raw transcript of one entry", async () => {
  open();
  await screen.findByText("open main.rs");
  fireEvent.click(screen.getAllByRole("button", { name: "Copy" })[0]);
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("history_copy", { id: 2, raw: false }),
  );
  fireEvent.click(screen.getAllByRole("button", { name: "Copy raw" })[0]);
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("history_copy", { id: 2, raw: true }),
  );
  // The second entry's raw transcript equals its text, so it offers no second copy.
  expect(screen.getAllByRole("button", { name: "Copy raw" })).toHaveLength(1);
  expect(await screen.findByText("Copied the raw transcript")).toBeTruthy();
});
it("deletes one entry and reloads the list", async () => {
  open();
  await screen.findByText("open main.rs");
  fireEvent.click(screen.getAllByRole("button", { name: "Delete" })[0]);
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("history_delete", { id: 2 }),
  );
  await waitFor(() => expect(screen.queryByText("open main.rs")).toBeNull());
  expect(screen.getByText("fetchUserProfile")).toBeTruthy();
});
it("clears every entry only after a second click confirms it", async () => {
  open();
  await screen.findByText("open main.rs");
  fireEvent.click(screen.getByRole("button", { name: "Clear history" }));
  expect(ipc).not.toHaveBeenCalledWith("history_clear", undefined);
  expect(screen.getByText("Delete all 2 entries?")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByText("Delete all 2 entries?")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Clear history" }));
  fireEvent.click(screen.getByRole("button", { name: "Delete all" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenCalledWith("history_clear", undefined),
  );
  expect(await screen.findByText("No results saved yet")).toBeTruthy();
});
it("searches from the first page", async () => {
  open();
  await screen.findByText("open main.rs");
  fireEvent.change(screen.getByLabelText("Search history"), {
    target: { value: "FETCH" },
  });
  expect(await screen.findByText("1-1 of 1 matches")).toBeTruthy();
  expect(screen.queryByText("open main.rs")).toBeNull();
  expect(ipc).toHaveBeenLastCalledWith("history_page", {
    query: "FETCH",
    offset: 0,
    limit: 50,
  });
});
it("moves between pages of 50", async () => {
  items = Array.from({ length: 60 }, (_, i) =>
    entry(60 - i, `entry ${60 - i}`),
  );
  open({ revision: 60 });
  expect(await screen.findByText("1-50 of 60 entries")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(await screen.findByText("51-60 of 60 entries")).toBeTruthy();
  expect(ipc).toHaveBeenLastCalledWith(
    "history_page",
    expect.objectContaining({ offset: 50 }),
  );
  expect(
    (screen.getByRole("button", { name: "Next" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});
it("reloads when a new result is saved", async () => {
  const view = open();
  await screen.findByText("open main.rs");
  items = [entry(3, "brand new result"), ...items];
  view.rerender(<History enabled={true} revision={3} onSettings={vi.fn()} />);
  expect(await screen.findByText("brand new result")).toBeTruthy();
});
it("shows a failed load", async () => {
  ipc.mockRejectedValue("Cannot read history");
  open();
  expect(await screen.findByRole("alert")).toBeTruthy();
  expect(screen.getByText("Cannot read history")).toBeTruthy();
});
