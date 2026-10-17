import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AiStatus } from "../types";
import AiPanel from "./AiPanel";

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

function status(web: AiStatus["web"] = null): AiStatus {
  return {
    listening: true,
    error: null,
    last_activity_secs: null,
    last_ai: null,
    activity: [],
    bridge_path: "npt-mcp",
    bridge_found: true,
    desktop_config_path: "",
    desktop_configured: false,
    claude_code_command: "",
    codex_config_path: "",
    codex_configured: false,
    codex_command: "",
    generic_config: "",
    generic_config_core: "",
    web,
  };
}

function show(s: AiStatus, onWeb = vi.fn()) {
  render(
    <AiPanel
      status={s}
      onInstallClaudeDesktop={() => {}}
      onInstallCodex={() => {}}
      onWeb={onWeb}
      onClose={() => {}}
      onCopyReport={() => {}}
    />,
  );
  fireEvent.change(screen.getByLabelText("Which AI app"), { target: { value: "web" } });
  return onWeb;
}

describe("AiPanel web address", () => {
  it("turns web mode on, with the short tool list if chosen", () => {
    const onWeb = show(status());
    fireEvent.click(screen.getByLabelText("Short tool list for small models"));
    fireEvent.click(screen.getByLabelText("Let AI apps connect with a web address"));
    expect(onWeb).toHaveBeenCalledWith(true, true);
  });

  it("shows the address and key to copy while it is on, and turns it off", () => {
    const url = "http://127.0.0.1:5000/mcp";
    const onWeb = show(status({ url, key: "k".repeat(32), url_with_key: `${url}?key=${"k".repeat(32)}`, core: false }));
    expect(screen.getByLabelText("Web address").textContent).toBe(url);
    expect(screen.getByLabelText("Secret key").textContent).toBe("k".repeat(32));
    expect(screen.getByLabelText("Address with key").textContent).toContain("?key=");
    fireEvent.click(screen.getByLabelText("Let AI apps connect with a web address"));
    expect(onWeb).toHaveBeenCalledWith(false, false);
  });
});
