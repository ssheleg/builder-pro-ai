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
vi.mock("./SkillsTab", () => ({
  SkillsTab: () => <div data-testid="marker-skills" />,
}));
vi.mock("./InvocationLog", () => ({
  InvocationLog: () => <div data-testid="marker-log" />,
}));
vi.mock("./ArtifactsTab", () => ({
  ArtifactsTab: () => <div data-testid="marker-artifacts" />,
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

  it("renders the six tabs, defaulting to Servers", () => {
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

  it("switching to Tools mounts ToolsBrowser, unmounting ServersTab", () => {
    render(<ExtPanel />);
    fireEvent.click(screen.getByTestId("ext-tab-tools"));
    expect(screen.getByTestId("marker-tools")).toBeTruthy();
    expect(screen.queryByTestId("marker-servers")).toBeNull();
  });

  it("switching to Connectors mounts ConnectorsTab, unmounting ServersTab (S-EXT §8, T13b)", () => {
    render(<ExtPanel />);
    fireEvent.click(screen.getByTestId("ext-tab-connectors"));
    expect(screen.getByTestId("marker-connectors")).toBeTruthy();
    expect(screen.queryByTestId("marker-servers")).toBeNull();
  });

  it("switching to Skills mounts SkillsTab, unmounting ServersTab (S-EXT §8, D11, T17)", () => {
    render(<ExtPanel />);
    fireEvent.click(screen.getByTestId("ext-tab-skills"));
    expect(screen.getByTestId("marker-skills")).toBeTruthy();
    expect(screen.queryByTestId("marker-servers")).toBeNull();
  });

  it("switching to Log mounts InvocationLog, unmounting ServersTab (S-EXT §8, T18)", () => {
    render(<ExtPanel />);
    fireEvent.click(screen.getByTestId("ext-tab-log"));
    expect(screen.getByTestId("marker-log")).toBeTruthy();
    expect(screen.queryByTestId("marker-servers")).toBeNull();
  });

  it("switching to Artifacts mounts ArtifactsTab, unmounting ServersTab (S-EXT §8, T18)", () => {
    render(<ExtPanel />);
    fireEvent.click(screen.getByTestId("ext-tab-artifacts"));
    expect(screen.getByTestId("marker-artifacts")).toBeTruthy();
    expect(screen.queryByTestId("marker-servers")).toBeNull();
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
