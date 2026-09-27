/**
 * The commit message editor.
 *
 * The editor sits at the top of the right panel so staging and committing stay
 * one continuous flow. Enter submits, Shift+Enter inserts a newline, and the
 * square split button to its right chooses between a new commit and an amend,
 * which is then the default for the session.
 */

import { useState } from "react";

import { Icon } from "../../components/Icon";
import { Menu, TextArea, type MenuItemSpec } from "../../components/controls";
import type { CommitActionPreference } from "../../bridge/types";
import { useStore, type RepoState } from "../../app/store";
import { t } from "../../i18n/strings";

export function CommitPanel({ repo }: { repo: RepoState }) {
  const translate = useStore((state) => state.t);
  const preference = useStore((state) => state.config.view.commit_action);
  const setView = useStore((state) => state.setView);
  const runAction = useStore((state) => state.runAction);
  const [message, setMessageText] = useState("");

  // Amending rewrites the previous commit, so either kind of staged change is
  // enough; a plain commit needs something staged.
  const hasStaged = repo.files.some(isStagedFile);
  const canCommit = !repo.busy && hasStaged && message.trim().length > 0;

  const submit = () => {
    if (!canCommit) {
      return;
    }
    void runAction(repo.id, {
      action: "commit",
      message,
      amend: preference === "amend",
    });
    setMessageText("");
  };

  // The menu offers two modes, so it marks the one in effect: opening it and
  // reading it should say what the button will do.
  const actionItems: MenuItemSpec[] = [
    {
      id: "commit",
      label: t(translate, "commit-action-commit"),
      checked: preference === "commit",
      onSelect: () => {
        void setView({ commit_action: "commit" });
      },
    },
    {
      id: "amend",
      label: t(translate, "commit-action-amend"),
      checked: preference === "amend",
      onSelect: () => {
        void setView({ commit_action: "amend" });
      },
    },
  ];

  const label =
    preference === "amend"
      ? t(translate, "commit-amend-btn")
      : t(translate, "commit-btn");

  return (
    <div className="commit-panel" data-testid="commit-panel">
      <div className="commit-panel__header">
        <span>{t(translate, "commit-title")}</span>
      </div>
      <div className="commit-panel__body">
        <TextArea
          value={message}
          onChange={setMessageText}
          placeholder={t(translate, "commit-placeholder")}
          disabled={repo.busy}
          minRows={2}
          maxRows={5}
          onSubmit={submit}
          onEscape={() => setMessageText("")}
          testId="commit-message"
        />
        <div className="commit-panel__actions">
          <button
            type="button"
            className="tool-button tool-button--primary"
            style={{ flex: "1 1 auto", justifyContent: "center" }}
            disabled={!canCommit}
            data-testid="commit-submit"
            onClick={submit}
          >
            {label}
          </button>
          {/* A one-pixel divider keeps the split button reading as one control. */}
          <div className="commit-panel__divider" />
          <Menu items={actionItems} testId="commit-mode" align="end">
            <button
              type="button"
              className="tool-button tool-button--primary commit-panel__mode"
              disabled={repo.busy}
              data-testid="commit-mode-trigger"
              aria-label={t(translate, "commit-action-amend")}
            >
              <Icon name="chevron-down" size={12} />
            </button>
          </Menu>
        </div>
      </div>
    </div>
  );
}

function isStagedFile(file: RepoState["files"][number]): boolean {
  return file.index !== " " && file.index !== "?";
}

/** The two choices the split button offers, also used by the settings page. */
export const COMMIT_ACTIONS: { value: CommitActionPreference; key: string }[] = [
  { value: "commit", key: "commit-action-commit" },
  { value: "amend", key: "commit-action-amend" },
];
