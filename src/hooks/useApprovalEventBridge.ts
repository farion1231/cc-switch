import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Event name emitted by the Rust pairing layer. Mirrors `remote::events::APPROVAL_EVENT`. */
export const APPROVAL_EVENT = "remote-approval-required";

/**
 * Mirrors the Rust `SourceKind` enum (`events.rs`, serde `camelCase`).
 *
 * The variants are **lowerCamelCase on the wire** (`rename_all = "camelCase"`
 * applies to enum variants too): `Tailnet` serializes as `"tailnet"`,
 * `NotInTailnet` as `"notInTailnet"`. Writing them PascalCase here would type a
 * value that never arrives.
 *
 * `notInTailnet` is a *deterministic negative* -- tailscale answered "peer not
 * found" -- while `unavailable` means "could not be determined". The dialog
 * renders the two differently, so they must not be collapsed into one value.
 */
export type SourceKind =
  | "tailnet"
  | "loopback"
  | "notInTailnet"
  | "unavailable";

/** Mirrors the Rust `ApprovalEvidence` struct (`events.rs`, serde `camelCase`). */
export interface ApprovalEvidence {
  sourceKind: SourceKind;
  nodeName?: string | null;
  nodeStableId?: string | null;
  loginName?: string | null;
  isForeignAccount: boolean;
  peerAddr: string;
}

/**
 * Mirrors the Rust `ApprovalEvent` struct (`events.rs`, serde `camelCase`).
 *
 * These key names are the wire contract: the Rust side derives its serialization
 * from `#[serde(rename_all = "camelCase")]`, so a rename here without a matching
 * change there (or the reverse) yields `undefined` on every field. That is not a
 * hypothetical -- `userCode` read as `user_code` used to throw inside the
 * dialog's render on every approval request, and no test caught it because the
 * component test built its fixture with the same wrong key names. A cross
 * language gate now pins the two sides together (see
 * `src-tauri/tests/approval_event_contract_gate.rs`).
 */
export interface ApprovalEvent {
  requestId: string;
  clientName: string;
  userCode: string;
  evidence: ApprovalEvidence;
}

/**
 * Subscribe to pairing approval events.
 *
 * Same dispose-safe `listen` pattern as `useUsageEventBridge`: the callback is
 * held in a ref so a caller passing an inline arrow does not re-subscribe on
 * every render, and a listener registered after unmount is torn down immediately.
 */
export function useApprovalEventBridge(
  onEvent: (event: ApprovalEvent) => void,
) {
  const handlerRef = useRef(onEvent);
  handlerRef.current = onEvent;

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;

    (async () => {
      const off = await listen<ApprovalEvent>(APPROVAL_EVENT, (e) => {
        handlerRef.current(e.payload);
      });

      if (disposed) {
        off();
      } else {
        unlisten = off;
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
}
