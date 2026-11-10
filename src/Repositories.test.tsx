import { afterEach, beforeEach, it, expect, vi } from "vitest";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
} from "@testing-library/react";
import { Repositories } from "./Repositories";
import type { Repository } from "./types";
const ipc = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc }));
const repo = (id: string, root: string): Repository => ({
  id,
  root,
  selected: true,
  exclusions: [],
});
const repos = [
  repo("a", "/work/conch"),
  repo("b", "/work/api-server"),
  repo("c", "/work/dotfiles"),
];
const NOW = 1_700_000_000;
const indexes: [
  string,
  { available: boolean; incomplete: boolean; skipped: number; entries: number },
  number,
][] = [
  [
    "a",
    { available: true, incomplete: false, skipped: 0, entries: 1284 },
    NOW - 120,
  ],
  [
    "b",
    { available: true, incomplete: true, skipped: 12, entries: 40 },
    NOW - 7200,
  ],
  [
    "c",
    { available: false, incomplete: false, skipped: 0, entries: 0 },
    NOW - 60,
  ],
];
const props = () => ({
  repositories: repos,
  indexes,
  now: NOW,
  refreshing: false,
  onChange: vi.fn(),
  onChoose: vi.fn(),
  onRefresh: vi.fn(),
});
beforeEach(() => {
  ipc.mockReset();
  ipc.mockResolvedValue({ total: 0, items: [] });
});
afterEach(cleanup);
it("shows each repository with its name, path, state and term count", () => {
  render(<Repositories {...props()} />);
  expect(screen.getByText("conch")).toBeTruthy();
  expect(screen.getByText("/work/conch")).toBeTruthy();
  expect(screen.getByText("Ready")).toBeTruthy();
  expect(screen.getByText(/1,284 terms - updated 2 min ago/)).toBeTruthy();
  expect(screen.getByText("Partial index")).toBeTruthy();
  expect(screen.getByText(/updated 2 hr ago/)).toBeTruthy();
  expect(screen.getByText("Folder not found")).toBeTruthy();
});
it("explains the empty state and still offers to add a repository", () => {
  const p = props();
  render(<Repositories {...p} repositories={[]} />);
  expect(screen.getByText("No repositories yet")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Add repository" }));
  expect(p.onChoose).toHaveBeenCalledOnce();
});
it("removes a repository from the draft", () => {
  const p = props();
  render(<Repositories {...p} />);
  fireEvent.click(screen.getByRole("button", { name: "Remove conch" }));
  expect(p.onChange).toHaveBeenCalledWith([repos[1], repos[2]]);
});
it("edits exclusions one pattern per line", () => {
  const p = props();
  render(<Repositories {...p} />);
  fireEvent.click(
    screen.getByRole("button", { name: "Exclusions for api-server" }),
  );
  fireEvent.change(
    screen.getByRole("textbox", { name: "Extra exclusions for api-server" }),
    {
      target: { value: "vendor/\n\n*.generated.ts" },
    },
  );
  expect(p.onChange).toHaveBeenCalledWith([
    repos[0],
    { ...repos[1], exclusions: ["vendor/", "*.generated.ts"] },
    repos[2],
  ]);
});
it("adds a repository by typed path", () => {
  const p = props();
  render(<Repositories {...p} />);
  fireEvent.click(screen.getByRole("button", { name: "Enter a path instead" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Repository path" }), {
    target: { value: "  /work/new  " },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add path" }));
  const next = p.onChange.mock.calls[0][0] as Repository[];
  expect(next).toHaveLength(4);
  expect(next[3]).toMatchObject({
    root: "/work/new",
    selected: true,
    exclusions: [],
  });
});
it("refreshes the indexes", () => {
  const p = props();
  render(<Repositories {...p} />);
  fireEvent.click(screen.getByRole("button", { name: "Refresh indexes" }));
  expect(p.onRefresh).toHaveBeenCalledOnce();
});
it("opens the indexed terms for an indexed repository and returns", async () => {
  render(<Repositories {...props()} />);
  fireEvent.click(screen.getByRole("button", { name: "Terms for conch" }));
  expect(await screen.findByText("No terms indexed yet")).toBeTruthy();
  expect(ipc).toHaveBeenCalledWith(
    "index_terms",
    expect.objectContaining({ repository: "a" }),
  );
  fireEvent.click(screen.getByRole("button", { name: /Repositories/ }));
  expect(screen.getByText("Ready")).toBeTruthy();
});
it("does not offer terms for a repository that has no index yet", () => {
  render(
    <Repositories {...props()} repositories={[repo("new", "/work/new")]} />,
  );
  expect(
    (screen.getByRole("button", { name: "Terms for new" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(screen.getByText("Not indexed yet")).toBeTruthy();
});
it("resets an open exclusions editor when the draft is discarded", () => {
  const p = props();
  const edited = [repos[0], { ...repos[1], exclusions: ["vendor/"] }, repos[2]];
  const { rerender } = render(<Repositories {...p} repositories={edited} />);
  fireEvent.click(
    screen.getByRole("button", { name: "Exclusions for api-server" }),
  );
  const box = () =>
    screen.getByRole("textbox", {
      name: "Extra exclusions for api-server",
    }) as HTMLTextAreaElement;
  expect(box().value).toBe("vendor/");
  rerender(<Repositories {...p} repositories={repos} />);
  expect(box().value).toBe("");
});
it("reports symbol extractions skipped for a ready repository", () => {
  render(
    <Repositories
      {...props()}
      indexes={[
        [
          "a",
          { available: true, incomplete: false, skipped: 7, entries: 10 },
          NOW - 60,
        ],
      ]}
    />,
  );
  expect(screen.getByText("Ready")).toBeTruthy();
  expect(screen.getByText(/7 symbol extractions skipped/)).toBeTruthy();
});
it("reloads the open terms when the repository is indexed again", async () => {
  const p = props();
  const { rerender } = render(<Repositories {...p} />);
  fireEvent.click(screen.getByRole("button", { name: "Terms for conch" }));
  await screen.findByText("No terms indexed yet");
  const before = ipc.mock.calls.length;
  rerender(
    <Repositories
      {...p}
      indexes={[
        [
          "a",
          { available: true, incomplete: false, skipped: 0, entries: 1300 },
          NOW + 30,
        ],
        indexes[1],
        indexes[2],
      ]}
    />,
  );
  await waitFor(() => expect(ipc.mock.calls.length).toBe(before + 1));
});
