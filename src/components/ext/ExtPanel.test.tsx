// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";

vi.mock("./ServersTab", () => ({
  ServersTab: () => <div data-testid="marker-servers" />,
}));
vi.mock("./ToolsBrowser", () => ({
  ToolsBrowser: () => <div data-testid="marker-tools" />,
}));
vi.mock("./ConnectorsTab", () => ({
  ConnectorsTab: () => <div data-testid="marker-connectors" />,
}));

const mcpListServersMock = vi.fn();
vi.mock("../../ipc/orchd", () => ({
  mcpListServers: (...a: unknown[]) => mcpListServersMock(...a),
}));

import { ExtPanel } from "./ExtPanel";
import { useAppStore } from "../../store/store";

afterEach(cleanup);
beforeEach(() => {
  mcpListServersMock.mockReset().mockResolvedValue([]);
  useAppStore.setState(
    { mcpServers: [], mcpToolsByServer: {}, mcpArtifacts: [], accounts: [], orchdDown: false },
    false,
  );
});

describe("ExtPanel", () => {
  it("mounts refreshMcpServers (mcpListServers) on mount", () => {
    render(<ExtPanel />);
    expect(mcpListServersMock).toHaveBeenCalledWith(null);
  });

  it("renders the six tabs, defaulting to «Серверы»", () => {
    render(<ExtPanel />);
    expect(screen.getByTestId("ext-tab-servers")).toBeTruthy();
    expect(screen.getByTestId("ext-tab-tools")).toBeTruthy();
    expect(screen.getByTestId("ext-tab-connectors")).toBeTruthy();
    expect(screen.getByTestId("ext-tab-log")).toBeTruthy();
    expect(screen.getByTestId("ext-tab-artifacts")).toBeTruthy();
    expect(screen.getByTestId("ext-tab-skills")).toBeTruthy();
    expect(screen.getByTestId("marker-servers")).toBeTruthy();
    expect(screen.queryByTestId("marker-tools")).toBeNull();
  });

  it("switching to «Инструменты» mounts ToolsBrowser, unmounting ServersTab", () => {
    render(<ExtPanel />);
    fireEvent.click(screen.getByTestId("ext-tab-tools"));
    expect(screen.getByTestId("marker-tools")).toBeTruthy();
    expect(screen.queryByTestId("marker-servers")).toBeNull();
  });

  it("switching to «Коннекторы» mounts ConnectorsTab, unmounting ServersTab (S-EXT §8, T13b)", () => {
    render(<ExtPanel />);
    fireEvent.click(screen.getByTestId("ext-tab-connectors"));
    expect(screen.getByTestId("marker-connectors")).toBeTruthy();
    expect(screen.queryByTestId("marker-servers")).toBeNull();
  });

  it("the not-yet-built tabs render a «скоро» stub", () => {
    render(<ExtPanel />);
    for (const key of ["log", "artifacts", "skills"]) {
      fireEvent.click(screen.getByTestId(`ext-tab-${key}`));
      expect(screen.getByTestId("ext-tab-stub")).toBeTruthy();
    }
  });

  it("renders the OrchdDownBanner while orchdDown is true", () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<ExtPanel />);
    expect(screen.getByTestId("orchd-down-banner")).toBeTruthy();
  });

  it("hides the OrchdDownBanner while orchdDown is false", () => {
    render(<ExtPanel />);
    expect(screen.queryByTestId("orchd-down-banner")).toBeNull();
  });
});
