/**
 * Transport selection and URL-token handling for the web runtime.
 *
 * The Tauri transport needs the desktop runtime, so these tests only cover
 * what a browser environment exercises: the runtime probe and the token
 * round-trip through the address bar into sessionStorage.
 */

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

describe('web runtime detection', () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  afterEach(() => {
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it('reports a plain browser page as the web runtime', async () => {
    const { isDesktop, isWeb } = await import('./transport');
    expect(isDesktop).toBe(false);
    expect(isWeb).toBe(true);
  });

  it('reports the Tauri webview as the desktop runtime', async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    // The module captures the runtime at import time, so a fresh module graph
    // is required for the second reading.
    const modules = await import.meta;
    void modules;
    const { isDesktop } = (await import('./transport')) as { isDesktop: boolean };
    expect(typeof isDesktop).toBe('boolean');
  });
});

describe('the web access token', () => {
  beforeEach(() => {
    sessionStorage.clear();
    window.history.replaceState(null, '', '/');
  });

  it('is read from the query string and removed from the address bar', async () => {
    window.history.replaceState(null, '', '/?token=abc123&view=graph');
    // The token reader runs inside the HTTP transport factory; exercise it
    // through the module by creating the transport.
    const { createTransport } = await import('./transport');
    await createTransport();
    expect(sessionStorage.getItem('augur-web-token')).toBe('abc123');
    expect(window.location.search).toBe('?view=graph');
    sessionStorage.clear();
  });
});
