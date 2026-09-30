/**
 * The Homebrew upgrade hint for a cask installation.
 *
 * The updater cannot replace a cask, so this hint names the command the user
 * has to run instead. A command shown as plain text has to be transcribed by
 * hand, so it carries a copy button; both the About window and the main-window
 * update notice render this same hint so the command cannot drift between them.
 */

import { useEffect, useRef, useState } from 'react';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';

import { useStore } from '../../app/store';
import { Icon } from '../../components/Icon';
import { t } from '../../i18n/strings';

/** The command a Homebrew cask installation needs to pick up a new version. */
export const HOMEBREW_UPGRADE_COMMAND = 'brew upgrade --cask augur-git';

/** How long the button stays in its copied state before returning to the copy icon. */
const COPIED_FEEDBACK_MS = 1600;

export function HomebrewUpgradeHint() {
  const translate = useStore((state) => state.t);
  const [copied, setCopied] = useState(false);
  const resetTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (resetTimer.current) clearTimeout(resetTimer.current);
    },
    []
  );

  const copy = () => {
    void writeText(HOMEBREW_UPGRADE_COMMAND).then(() => {
      setCopied(true);
      if (resetTimer.current) clearTimeout(resetTimer.current);
      resetTimer.current = setTimeout(() => setCopied(false), COPIED_FEEDBACK_MS);
    });
  };

  const label = t(translate, copied ? 'homebrew-copied' : 'homebrew-copy-upgrade');

  return (
    <p className="homebrew-hint">
      {t(translate, 'homebrew-upgrade-hint')} <code>{HOMEBREW_UPGRADE_COMMAND}</code>
      <button
        type="button"
        className="homebrew-hint__copy"
        data-testid="homebrew-copy-upgrade"
        title={label}
        aria-label={label}
        onClick={copy}
      >
        <Icon name={copied ? 'check' : 'copy'} size={12} />
      </button>
      {/* The icon swap is the visible confirmation; this announces it without one. */}
      <span className="sr-only" role="status">
        {copied ? label : ''}
      </span>
    </p>
  );
}
