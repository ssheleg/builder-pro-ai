// @vitest-environment jsdom
import { describe, it, expect, afterEach, beforeEach } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import { StorageBanner } from "./StorageBanner";
import { useAppStore } from "../store/store";
import { strings } from "../strings";

afterEach(cleanup);
beforeEach(() => {
  useAppStore.setState({ storageStatus: null }, false);
});

describe("StorageBanner (spec D3, BL-94)", () => {
  it("renders nothing before the first fetch (storageStatus null)", () => {
    render(<StorageBanner />);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("renders nothing for the healthy persistent mode", () => {
    act(() =>
      useAppStore.setState(
        { storageStatus: { storageMode: "persistent", quarantinedPath: null } },
        false,
      ),
    );
    render(<StorageBanner />);
    expect(screen.queryByTestId("storage-banner")).toBeNull();
  });

  it("renders the in-memory-fallback copy (D3): changes will NOT survive a restart", () => {
    act(() =>
      useAppStore.setState(
        { storageStatus: { storageMode: "in_memory_fallback", quarantinedPath: null } },
        false,
      ),
    );
    render(<StorageBanner />);
    expect(screen.getByTestId("storage-banner").textContent).toBe(strings.storage.inMemory);
  });

  it("renders the recovered-from-corruption copy (D3) including the quarantined path", () => {
    act(() =>
      useAppStore.setState(
        {
          storageStatus: {
            storageMode: "recovered_from_corruption",
            quarantinedPath: "/tmp/orchd.db.corrupt-42",
          },
        },
        false,
      ),
    );
    render(<StorageBanner />);
    const banner = screen.getByTestId("storage-banner");
    expect(banner.textContent).toBe(strings.storage.recovered("/tmp/orchd.db.corrupt-42"));
    expect(banner.textContent).toContain("/tmp/orchd.db.corrupt-42");
  });

  it("reactively appears when the mode degrades and disappears when it returns to persistent", () => {
    render(<StorageBanner />);
    expect(screen.queryByRole("alert")).toBeNull();
    act(() =>
      useAppStore.setState(
        { storageStatus: { storageMode: "in_memory_fallback", quarantinedPath: null } },
        false,
      ),
    );
    expect(screen.getByRole("alert")).toBeTruthy();
    act(() =>
      useAppStore.setState(
        { storageStatus: { storageMode: "persistent", quarantinedPath: null } },
        false,
      ),
    );
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
