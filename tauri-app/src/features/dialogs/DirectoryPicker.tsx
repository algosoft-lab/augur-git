/**
 * Server-backed directory picker for the web runtime.
 *
 * A browser cannot hand the server a filesystem path, so picking a folder
 * walks the server's directories through /api/browse. The dialog is mounted
 * once per page and woken by the platform layer's pick request event; on the
 * desktop it never fires because the native dialog answers first.
 */

import { useEffect, useState } from 'react';

import { Icon } from '../../components/Icon';
import { DialogCard, TextInput, ToolButton } from '../../components/controls';
import { useStore } from '../../app/store';
import {
  PICK_DIRECTORY_EVENT,
  browseServerDirectory,
  type DirectoryListing
} from '../../platform';

interface PickRequest {
  title: string;
  resolve: (path: string | null) => void;
}

interface PickerState {
  request: PickRequest;
  listing: DirectoryListing | null;
  error: string | null;
}

export function DirectoryPicker() {
  const translate = useStore((state) => state.t);
  const [state, setState] = useState<PickerState | null>(null);
  const [manualPath, setManualPath] = useState('');

  useEffect(() => {
    const onPick = (event: Event) => {
      const request = (event as CustomEvent<PickRequest>).detail;
      setState({ request, listing: null, error: null });
      void browseServerDirectory('')
        .then((listing) => setState((current) => (current ? { ...current, listing } : current)))
        .catch((error: unknown) =>
          setState((current) => (current ? { ...current, error: String(error) } : current))
        );
    };
    window.addEventListener(PICK_DIRECTORY_EVENT, onPick);
    return () => window.removeEventListener(PICK_DIRECTORY_EVENT, onPick);
  }, []);

  if (!state) {
    return null;
  }

  const finish = (path: string | null) => {
    state.request.resolve(path);
    setState(null);
  };

  const open = async (path: string) => {
    setState((current) => (current ? { ...current, error: null } : current));
    try {
      const listing = await browseServerDirectory(path);
      setState((current) => (current ? { ...current, listing } : current));
    } catch (error) {
      setState((current) => (current ? { ...current, error: String(error) } : current));
    }
  };

  const listing = state.listing;

  return (
    <DialogCard
      title={state.request.title}
      subtitle={listing ? listing.path : undefined}
      testId="directory-picker"
      body={
        <>
        <div className="directory-picker__manual">
          <TextInput
            value={manualPath}
            onChange={setManualPath}
            placeholder={listing ? listing.root : '/'}
            ariaLabel={translate('clone-parent-label')}
            onSubmit={() => void open(manualPath)}
          />
          <ToolButton onClick={() => void open(manualPath)} disabled={!manualPath.trim()}>
            {translate('dialog-confirm')}
          </ToolButton>
        </div>
        <div className="directory-picker__list" role="listbox" aria-label={state.request.title}>
          {state.error ? <p className="dialog__error">{state.error}</p> : null}
          {listing?.parent ? (
            <button
              type="button"
              className="directory-picker__row"
              onClick={() => void open(listing.parent as string)}
            >
              <Icon name="chevron-up" size={12} /> ..
            </button>
          ) : null}
          {(listing?.entries ?? []).map((entry) => (
            <button
              key={entry.path}
              type="button"
              className="directory-picker__row"
              onClick={() => void open(entry.path)}
            >
              <Icon name="chevron-right" size={12} /> {entry.name}
            </button>
          ))}
          {listing && listing.entries.length === 0 ? (
            <p className="dialog__hint">{translate('welcome-drop-hint')}</p>
          ) : null}
        </div>
        </>
      }
      footer={
        <>
          <ToolButton onClick={() => finish(null)}>{translate('dialog-cancel')}</ToolButton>
          <ToolButton
            onClick={() => finish(listing ? listing.path : null)}
            disabled={!listing}
            testId="directory-picker-choose"
          >
            {translate('dialog-confirm')}
          </ToolButton>
        </>
      }
      onBackdrop={() => finish(null)}
    />
  );
}
