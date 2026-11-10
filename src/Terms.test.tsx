import { afterEach, beforeEach, it, expect, vi } from "vitest";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
} from "@testing-library/react";
import { RepositoryTerms } from "./Terms";
const ipc = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc }));
const TOTAL = 150;
const page = (offset: number, limit: number, total = TOTAL) => ({
  total,
  items: Array.from(
    { length: Math.max(0, Math.min(limit, total - offset)) },
    (_, i) => ({
      text: `term${offset + i}`,
      repository: "r1",
      file: `src/file${offset + i}.ts`,
    }),
  ),
});
beforeEach(() => {
  ipc.mockReset();
  ipc.mockImplementation(
    async (name: string, args: { offset: number; limit: number }) =>
      name === "index_terms" ? page(args.offset, args.limit) : undefined,
  );
});
afterEach(cleanup);
const open = () =>
  render(<RepositoryTerms id="r1" name="api-server" onBack={vi.fn()} />);
it("loads the first page of 100 terms with the total", async () => {
  open();
  expect(await screen.findByText("1-100 of 150 terms")).toBeTruthy();
  expect(ipc).toHaveBeenCalledWith("index_terms", {
    repository: "r1",
    query: "",
    kind: "all",
    offset: 0,
    limit: 100,
  });
  expect(screen.getByText("term0")).toBeTruthy();
  expect(screen.getByText("src/file0.ts")).toBeTruthy();
});
it("moves between pages and disables the ends", async () => {
  open();
  await screen.findByText("1-100 of 150 terms");
  const previous = screen.getByRole("button", {
    name: "Previous",
  }) as HTMLButtonElement;
  expect(previous.disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(await screen.findByText("101-150 of 150 terms")).toBeTruthy();
  expect(ipc).toHaveBeenLastCalledWith(
    "index_terms",
    expect.objectContaining({ offset: 100 }),
  );
  expect(
    (screen.getByRole("button", { name: "Next" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});
it("searches from the first page and highlights the match", async () => {
  open();
  await screen.findByText("1-100 of 150 terms");
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText("101-150 of 150 terms");
  ipc.mockImplementation(async (_name: string, args: { query: string }) => ({
    total: 1,
    items: [{ text: `fetch${args.query}`, repository: "r1", file: "a.ts" }],
  }));
  fireEvent.change(screen.getByRole("textbox", { name: "Search terms" }), {
    target: { value: "Fe" },
  });
  await waitFor(() =>
    expect(ipc).toHaveBeenLastCalledWith("index_terms", {
      repository: "r1",
      query: "Fe",
      kind: "all",
      offset: 0,
      limit: 100,
    }),
  );
  expect(await screen.findByText("1-1 of 1 matches")).toBeTruthy();
  const mark = document.querySelector("mark");
  expect(mark?.textContent?.toLowerCase()).toBe("fe");
});
it("filters to files or symbols", async () => {
  open();
  await screen.findByText("1-100 of 150 terms");
  fireEvent.click(screen.getByRole("button", { name: "Symbols" }));
  await waitFor(() =>
    expect(ipc).toHaveBeenLastCalledWith(
      "index_terms",
      expect.objectContaining({ kind: "symbols", offset: 0 }),
    ),
  );
  expect(
    screen
      .getByRole("button", { name: "Symbols" })
      .getAttribute("aria-pressed"),
  ).toBe("true");
});
it("says when nothing matches and returns to the list", async () => {
  const back = vi.fn();
  ipc.mockImplementation(async () => ({ total: 0, items: [] }));
  render(<RepositoryTerms id="r1" name="api-server" onBack={back} />);
  expect(await screen.findByText("No terms indexed yet")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: /Repositories/ }));
  expect(back).toHaveBeenCalledOnce();
});
it("keeps the page it is on until the search text changes", async () => {
  open();
  await screen.findByText("1-100 of 150 terms");
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText("101-150 of 150 terms");
  await new Promise((resolve) => setTimeout(resolve, 400));
  expect(screen.getByText("101-150 of 150 terms")).toBeTruthy();
  expect(ipc).toHaveBeenLastCalledWith(
    "index_terms",
    expect.objectContaining({ offset: 100 }),
  );
});
it("loads the terms again when the index is refreshed", async () => {
  const { rerender } = render(
    <RepositoryTerms
      id="r1"
      name="api-server"
      refreshed={1}
      onBack={vi.fn()}
    />,
  );
  await screen.findByText("1-100 of 150 terms");
  const before = ipc.mock.calls.length;
  rerender(
    <RepositoryTerms
      id="r1"
      name="api-server"
      refreshed={2}
      onBack={vi.fn()}
    />,
  );
  await waitFor(() => expect(ipc.mock.calls.length).toBe(before + 1));
});
it("keeps the range of the page it shows when the next page fails", async () => {
  open();
  await screen.findByText("1-100 of 150 terms");
  ipc.mockRejectedValueOnce("Cannot read index");
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(await screen.findByRole("alert")).toBeTruthy();
  expect(screen.getByText("1-100 of 150 terms")).toBeTruthy();
  expect(
    (screen.getByRole("button", { name: "Next" }) as HTMLButtonElement)
      .disabled,
  ).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(await screen.findByText("101-150 of 150 terms")).toBeTruthy();
});
