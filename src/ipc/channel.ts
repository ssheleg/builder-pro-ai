import { Channel } from "@tauri-apps/api/core";
import type { TerminalEvent } from "./types";

/**
 * Build a Tauri `Channel<TerminalEvent>` for `attach_session` (spec §6.2). The daemon-brokered
 * firehose (`Replay` then live `Output` frames — see `src-tauri/src/broker.rs::map_push`) arrives
 * here verbatim, one call to `onEvent` per frame, in delivery order.
 *
 * This layer only delivers the typed `TerminalEvent`; turning `data.content` / `data.bytes`
 * (`number[]`) into a `Uint8Array` for `term.write` — and keeping those bytes out of
 * React/Zustand state (spec §6.2) — is the terminal-manager's job (Task 21), not this one's.
 */
export function newTerminalChannel(
  onEvent: (e: TerminalEvent) => void,
): Channel<TerminalEvent> {
  const channel = new Channel<TerminalEvent>();
  channel.onmessage = (m: TerminalEvent) => onEvent(m);
  return channel;
}
