/**
 * The About window.
 *
 * A separate window rather than a settings section, matching the reference
 * application. It reports the identity, version, and build commit of this
 * application, where its data lives, and the command that opens a repository
 * from a terminal.
 */

import { useStore } from '../../app/store';
import { t, ta } from '../../i18n/strings';

export function AboutWindow() {
  const translate = useStore((state) => state.t);
  const build = useStore((state) => state.build);
  const storePaths = useStore((state) => state.storePaths);

  return (
    <div className="window-page">
      <div className="window-page__title" data-tauri-drag-region>
        {t(translate, 'about-title')}
      </div>
      <div className="about" data-testid="about">
        <img className="about__logo" src="/logo.svg" alt="" />
        <div className="about__name">{build?.name ?? 'Augur Git Tauri'}</div>
        <div className="about__tagline">{t(translate, 'about-tagline')}</div>
        <dl className="about__rows">
          <div className="about__row">
            <dt>{t(translate, 'about-version')}</dt>
            <dd data-testid="about-version">{build?.version ?? ''}</dd>
          </div>
          <div className="about__row">
            <dt>{t(translate, 'about-commit')}</dt>
            <dd data-testid="about-commit">{build?.commit ?? 'unknown'}</dd>
          </div>
          <div className="about__row">
            <dt>{t(translate, 'about-author')}</dt>
            <dd data-testid="about-author">{build?.authors ?? ''}</dd>
          </div>
          <div className="about__row">
            <dt>{t(translate, 'app-identifier')}</dt>
            <dd data-testid="about-identifier">{build?.identifier ?? ''}</dd>
          </div>
          <div className="about__row">
            <dt>{t(translate, 'cli-command')}</dt>
            <dd data-testid="about-cli">{build?.cli_command ?? ''}</dd>
          </div>
          <div className="about__row">
            <dt>{t(translate, 'about-platform')}</dt>
            <dd data-testid="about-platform">{build?.platform ?? ''}</dd>
          </div>
          {storePaths.map((path) => (
            <div className="about__row" key={path}>
              <dt>{t(translate, 'app-data-dir')}</dt>
              <dd title={path}>{path}</dd>
            </div>
          ))}
        </dl>
        <div className="about__hint">
          {ta(translate, 'about-cli-hint', {
            command: build?.cli_command ?? 'augurgit-tauri'
          })}
        </div>
      </div>
    </div>
  );
}
