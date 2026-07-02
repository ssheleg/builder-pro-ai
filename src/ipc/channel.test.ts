import { describe, it, expect, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => {
  class Channel<T> {
    onmessage: ((m: T) => void) | undefined;
  }
  return { Channel };
});

import { newTerminalChannel } from "./channel";
import type { TerminalEvent } from "./types";

describe("ipc/channel", () => {
  it("routes replay then output frames to the handler in order", () => {
    const received: TerminalEvent[] = [];
    const ch = newTerminalChannel((e) => received.push(e));
    const replay: TerminalEvent = {
      event: "replay",
      data: { cols: 80, rows: 24, content: [104, 105] },
    };
    const output: TerminalEvent = { event: "output", data: { bytes: [10] } };
    ch.onmessage?.(replay);
    ch.onmessage?.(output);
    expect(received).toEqual([replay, output]);
  });

  it("wires onmessage exactly to the provided handler", () => {
    const handler = vi.fn();
    const ch = newTerminalChannel(handler);
    const msg: TerminalEvent = { event: "output", data: { bytes: [65] } };
    ch.onmessage?.(msg);
    expect(handler).toHaveBeenCalledTimes(1);
    expect(handler).toHaveBeenCalledWith(msg);
  });
});
