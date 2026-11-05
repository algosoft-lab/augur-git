/**
 * The pending HTTPS credential requests.
 *
 * The backend broadcasts one event per prompt and waits for an answer keyed by
 * request id, so any surface that can answer may do so. The queue is shared
 * instead of per component for the same reason: the clone wizard answers
 * prompts while it is open, and the standalone credential dialog answers the
 * rest. A module-level store keeps both views of the same list.
 */

import { useSyncExternalStore } from 'react';

import * as ipc from '../../bridge/ipc';
import type { LfsCredentialRequest } from '../../bridge/types';

let pending: LfsCredentialRequest[] = [];
const listeners = new Set<() => void>();
let subscribed = false;

function notify(): void {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  if (!subscribed) {
    subscribed = true;
    void ipc
      .onLfsCredentialRequest((request) => {
        if (pending.some((entry) => entry.requestId === request.requestId)) return;
        pending = [...pending, request];
        notify();
      })
      .catch(() => {
        subscribed = false;
      });
  }
  return () => {
    listeners.delete(listener);
  };
}

export function useCredentialRequests(): LfsCredentialRequest[] {
  return useSyncExternalStore(
    subscribe,
    () => pending,
    () => pending
  );
}

export function dropCredentialRequest(requestId: string): void {
  const next = pending.filter((entry) => entry.requestId !== requestId);
  if (next.length === pending.length) return;
  pending = next;
  notify();
}
