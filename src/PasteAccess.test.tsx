import { it, expect, vi, afterEach } from "vitest";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
} from "@testing-library/react";
import { PasteAccess } from "./PasteAccess";
afterEach(cleanup);
it("keeps stale-grant repair explicit and scoped to Conch", async () => {
  const repair = vi.fn(async () => true);
  const open = vi.fn();
  render(
    <PasteAccess
      granted={false}
      path="/Applications/Conch.app"
      onOpen={open}
      onRepair={repair}
      onRestart={vi.fn()}
    />,
  );
  expect(repair).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "Enable automatic paste" }),
  );
  expect(open).toHaveBeenCalledOnce();
  expect(repair).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByText("Enabled in Settings but still not detected?"),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Repair Accessibility entry" }),
  );
  expect(repair).not.toHaveBeenCalled();
  expect(screen.getByText(/removes only Conch/)).toBeTruthy();
  fireEvent.click(
    screen.getByRole("button", { name: "Remove old entry and open Settings" }),
  );
  await waitFor(() => expect(repair).toHaveBeenCalledOnce());
});
it("updates to granted without another enable or repair action", () => {
  const props = {
    path: "/Applications/Conch.app",
    onOpen: vi.fn(),
    onRepair: vi.fn(async () => true),
    onRestart: vi.fn(),
  };
  const rendered = render(<PasteAccess granted={false} {...props} />);
  rendered.rerender(<PasteAccess granted {...props} />);
  expect(
    screen.getByText("Automatic paste is enabled for supported text fields."),
  ).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "Enable automatic paste" }),
  ).toBeNull();
});
it("offers restart without resetting access and disables recovery while recording", () => {
  const repair = vi.fn(async () => true);
  const restart = vi.fn();
  const rendered = render(
    <PasteAccess
      granted={false}
      onOpen={vi.fn()}
      onRepair={repair}
      onRestart={restart}
    />,
  );
  fireEvent.click(
    screen.getByText("Enabled in Settings but still not detected?"),
  );
  fireEvent.click(screen.getByRole("button", { name: "Restart Conch" }));
  expect(restart).toHaveBeenCalledOnce();
  expect(repair).not.toHaveBeenCalled();
  rendered.rerender(
    <PasteAccess
      granted={false}
      busy
      onOpen={vi.fn()}
      onRepair={repair}
      onRestart={restart}
    />,
  );
  expect(
    (screen.getByRole("button", { name: "Restart Conch" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});
