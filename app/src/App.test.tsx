import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import App from "./App";
import { createPreviewBackend } from "./backend";

afterEach(cleanup);

async function renderApp() {
  render(<App backend={createPreviewBackend()} />);
  return screen.findByLabelText("Tempo in BPM");
}

describe("App", () => {
  it("shows the default project", async () => {
    const tempo = (await renderApp()) as HTMLInputElement;
    expect(tempo.value).toBe("120");
    expect((screen.getByLabelText("Time signature") as HTMLSelectElement).value).toBe("4/4");
    expect(screen.getByText("Undo", { exact: false }).closest("button")?.disabled).toBe(true);
  });

  it("sends a tempo Command on Enter and can undo it", async () => {
    const tempo = await renderApp();
    fireEvent.change(tempo, { target: { value: "90" } });
    fireEvent.blur(tempo);
    const updated = (await screen.findByDisplayValue("90")) as HTMLInputElement;
    expect(updated).toBeTruthy();

    fireEvent.click(screen.getByText("Undo", { exact: false }));
    expect(await screen.findByDisplayValue("120")).toBeTruthy();
  });

  it("rejects non-numeric tempo", async () => {
    const tempo = await renderApp();
    fireEvent.change(tempo, { target: { value: "fast" } });
    fireEvent.blur(tempo);
    expect((await screen.findByRole("alert")).textContent).toContain("not a number");
  });

  it("toggles the test sound", async () => {
    await renderApp();
    fireEvent.click(screen.getByRole("button", { name: "Test sound" }));
    expect(await screen.findByText("Stop test sound")).toBeTruthy();
  });
});
