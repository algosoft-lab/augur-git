/**
 * The transport seam under the bridge.
 *
 * The desktop build talks to the Tauri runtime; the browser build talks to the
 * augur-server HTTP API and its event websocket. Both expose the same surface:
 * named commands with JSON arguments, and named events with JSON payloads, so
 * the rest of the bridge does not know which transport is underneath.
 */

export type UnlistenFn = () => void;

export interface Transport {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  /** Subscribe to a global event. */
  listen<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn>;
  /**
   * Subscribe to an event scoped to this window. On the web every tab is part
   * of one broadcast, so this is the same as a global subscription.
   */
  windowListen<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn>;
  /** Emit an event to the backend. */
  emit(event: string, payload: unknown): Promise<void>;
}

/** Whether this page runs inside the Tauri webview. */
export const isDesktop =
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Whether this page is served by the augur-server web frontend. */
export const isWeb = !isDesktop;

// ===== Tauri transport =====

async function tauriTransportFactory(): Promise<Transport> {
  const { invoke } = await import('@tauri-apps/api/core');
  const { emit, listen } = await import('@tauri-apps/api/event');
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  return {
    invoke: (command, args) => invoke(command, args),
    listen: (event, handler) =>
      listen(event, (event) => handler(event.payload as never)),
    windowListen: (event, handler) =>
      getCurrentWindow().listen(event, (event) => handler(event.payload as never)),
    emit: (event, payload) => emit(event, payload)
  };
}

// ===== HTTP transport =====

/** Read the access token from the URL and remember it for the session. */
function readToken(): string | null {
  const params = new URLSearchParams(window.location.search);
  const hash = new URLSearchParams(window.location.hash.replace(/^#/, ''));
  const token = params.get('token') ?? hash.get('token');
  if (token) {
    sessionStorage.setItem('augur-web-token', token);
    // Keep the token out of the address bar now that it is stored.
    params.delete('token');
    hash.delete('token');
    const search = params.toString();
    const hashText = window.location.hash.startsWith('#') && hash.toString()
      ? `#${hash.toString()}`
      : window.location.hash;
    window.history.replaceState(
      null,
      '',
      `${window.location.pathname}${search ? `?${search}` : ''}${hashText}`
    );
    return token;
  }
  return sessionStorage.getItem('augur-web-token');
}

let reconnectNoticeHandlers: (() => void)[] = [];

/**
 * Subscribe to a callback that fires when the event socket comes back after a
 * drop. A tab that missed events while offline must re-read the repository
 * state, so the store listens for this.
 */
export function onTransportResumed(handler: () => void): UnlistenFn {
  reconnectNoticeHandlers.push(handler);
  return () => {
    reconnectNoticeHandlers = reconnectNoticeHandlers.filter((other) => other !== handler);
  };
}

async function httpTransportFactory(): Promise<Transport> {
  const token = readToken();
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  let socket: WebSocket | null = null;
  let socketGeneration = 0;
  let everConnected = false;

  const dispatch = (event: string, payload: unknown) => {
    const handlers = listeners.get(event);
    if (handlers) {
      for (const handler of handlers) {
        handler(payload);
      }
    }
  };

  const connect = () => {
    const generation = ++socketGeneration;
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const query = token ? `?token=${encodeURIComponent(token)}` : '';
    const ws = new WebSocket(`${protocol}//${window.location.host}/api/events${query}`);
    socket = ws;
    ws.onmessage = (message) => {
      try {
        const frame = JSON.parse(String(message.data)) as { event: string; payload: unknown };
        dispatch(frame.event, frame.payload);
      } catch {
        // A malformed frame is dropped; the next one carries on.
      }
    };
    ws.onclose = () => {
      if (socket !== ws || generation !== socketGeneration) {
        return;
      }
      socket = null;
      // Reconnect with a capped backoff; the tab may outlive a server restart.
      const delay = Math.min(8000, 500 * Math.pow(2, Math.min(4, wsErrors++)));
      setTimeout(() => {
        if (generation === socketGeneration) {
          connect();
        }
      }, delay);
    };
    ws.onopen = () => {
      wsErrors = 0;
      // The first connection is not a recovery; only a reconnect means the
      // tab lost events and must resynchronize.
      if (!everConnected) {
        everConnected = true;
        return;
      }
      for (const handler of reconnectNoticeHandlers) {
        handler();
      }
    };
  };
  let wsErrors = 0;
  connect();

  const sharedListen = async <T>(
    event: string,
    handler: (payload: T) => void
  ): Promise<UnlistenFn> => {
    const set = listeners.get(event) ?? new Set();
    listeners.set(event, set as Set<(payload: unknown) => void>);
    set.add(handler as (payload: unknown) => void);
    return () => {
      set.delete(handler as (payload: unknown) => void);
    };
  };

  return {
    invoke: async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
      const response = await fetch(`/api/invoke/${command}`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          ...(token ? { Authorization: `Bearer ${token}` } : {})
        },
        body: JSON.stringify(args ?? {})
      });
      const text = await response.text();
      let value: unknown = null;
      try {
        value = text ? JSON.parse(text) : null;
      } catch {
        value = null;
      }
      if (!response.ok) {
        // The server returns the same { key, detail } shape as a rejected
        // Tauri invoke, so one error path serves both transports.
        throw value ?? { key: 'err-network', detail: `HTTP ${response.status}` };
      }
      return value as T;
    },
    listen: sharedListen,
    windowListen: sharedListen,
    emit: async () => {
      // The web frontend has no window-to-window events to emit; the commands
      // it needs are plain HTTP invocations.
    }
  };
}

/** The transport for this runtime, created once and shared. */
export async function createTransport(): Promise<Transport> {
  return isDesktop ? tauriTransportFactory() : httpTransportFactory();
}
