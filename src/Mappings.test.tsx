import { afterEach, it, expect, vi } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { Mappings } from "./Mappings";
import type { Mapping, Repository } from "./types";
afterEach(cleanup);
const repositories: Repository[] = [
  { id: "a", root: "/work/conch", exclusions: [] },
];
const mappings: Mapping[] = [
  { phrase: "git hub", replacement: "GitHub", repository: null, enabled: true },
  {
    phrase: "use effect",
    replacement: "useEffect",
    repository: "a",
    enabled: false,
  },
];
const setup = (list = mappings) => {
  const onChange = vi.fn();
  render(
    <Mappings
      mappings={list}
      repositories={repositories}
      onChange={onChange}
    />,
  );
  return onChange;
};
it("shows every mapping as one editable row", () => {
  setup();
  const phrases = screen.getAllByRole("textbox", { name: "Spoken phrase" });
  expect(phrases.map((p) => (p as HTMLInputElement).value)).toEqual([
    "git hub",
    "use effect",
  ]);
  const scopes = screen.getAllByRole("combobox", { name: "Scope" });
  expect((scopes[1] as HTMLSelectElement).value).toBe("a");
  expect(screen.getAllByRole("option", { name: "conch" })).toHaveLength(2);
});
it("edits the phrase, replacement, scope and enabled state of one row", () => {
  const onChange = setup();
  fireEvent.change(
    screen.getAllByRole("textbox", { name: "Spoken phrase" })[0],
    {
      target: { value: "get hub" },
    },
  );
  expect(onChange).toHaveBeenLastCalledWith([
    { ...mappings[0], phrase: "get hub" },
    mappings[1],
  ]);
  fireEvent.change(screen.getAllByRole("combobox", { name: "Scope" })[0], {
    target: { value: "a" },
  });
  expect(onChange).toHaveBeenLastCalledWith([
    { ...mappings[0], repository: "a" },
    mappings[1],
  ]);
  fireEvent.click(screen.getAllByRole("checkbox", { name: "Enabled" })[1]);
  expect(onChange).toHaveBeenLastCalledWith([
    mappings[0],
    { ...mappings[1], enabled: true },
  ]);
});
it("adds an empty global mapping and removes a row", () => {
  const onChange = setup();
  fireEvent.click(screen.getByRole("button", { name: "Add mapping" }));
  expect(onChange).toHaveBeenLastCalledWith([
    ...mappings,
    { phrase: "", replacement: "", repository: null, enabled: true },
  ]);
  fireEvent.click(screen.getAllByRole("button", { name: "Remove mapping" })[0]);
  expect(onChange).toHaveBeenLastCalledWith([mappings[1]]);
});
it("explains the empty state", () => {
  setup([]);
  expect(screen.getByText("No mappings yet")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Add mapping" })).toBeTruthy();
});
