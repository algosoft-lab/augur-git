/**
 * The commit message editor.
 *
 * The editor sits at the top of the right panel so staging and committing stay
 * one continuous flow. Enter submits, Shift+Enter inserts a newline, and the
 * action split button persists the chosen default.
 */

import { useState } from "react";

import { writeText } from "@tauri-apps/plugin-clipboard-manager";

import { Icon } from "../../components/Icon";
import { Menu, TextArea, type MenuItemSpec } from "../../components/controls";
import type { CommitActionPreference } from "../../bridge/types";
import { useStore, type RepoState } from "../../app/store";
import { t } from "../../i18n/strings";

export function CommitPanel({ repo }: { repo: RepoState }) {
  const translate = useStore((state) => state.t);
  const preference = useStore((state) => state.config.view.commit_action);
  const setCommitAction = useStore((state) => state.setView);
  const runAction = useStore((state) => state.runAction);
  const setMessage = useStore((state) => state.setMessage);
  const [message, setMessageText] = useState("");

  const hasStaged = repo.files.some((file) => file.index !== " " && file.index !== "?");
  const hasChanges = repo.files.length > 0;
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

  const actionItems: MenuItemSpec[] = [
    {
      id: "commit",
      label: t(translate, "commit-action-commit"),
      onSelect: () => {
        void setCommitAction({ commit_action: "commit" });
      },
    },
    {
      id: "amend",
      label: t(translate, "commit-action-amend"),
      onSelect: () => {
        void setCommitAction({ commit_action: "amend" });
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
          onSubmit={submit}
          testId="commit-message"
        />
        <div className="commit-panel__actions">
          <button
            type="button"
            className="tool-button tool-button--primary"
            style={{ flex: 1, justifyContent: "center" }}
            disabled={!canCommit}
            data-testid="commit-submit"
            onClick={submit}
          >
            {label}
          </button>
          <Menu items={actionItems} testId="commit-mode" align="end">
            <button
              type="button"
              className="tool-button tool-button--primary"
              style={{ width: 26, justifyContent: "center" }}
              disabled={repo.busy}
              data-testid="commit-mode-trigger"
              aria-label={t(translate, "commit-action-amend")}
            >
              <Icon name="chevron-down" size={12} />
            </button>
          </Menu>
        </div>
        {repo.hasConflicts ? (
          <div className="muted" style={{ fontSize: "0.7em" }}>
            {t(translate, "changes-action-conflict")}
          </div>
        ) : null}
        {!hasChanges ? null : (
          <button
            type="button"
            className="tool-button tool-button--compact"
            style={{ alignSelf: "flex-start" }}
            data-testid="commit-copy-commands"
            onClick={() => {
              // Copy the staged file list so a reviewer can paste it elsewhere.
              const lines = repo.files
                .filter((file) => file.index !== " " && file.index !== "?")
                .map((file) => file.path);
              void writeText(lines.join("\n")).then(() => {
                setMessage(repo.id, t(translate, "context-copied"), true);
              });
            }}
          >
            <Icon name="copy" size={11} /> {t(translate, "context-copied")}
          </button>
        )}
      </div>
    </div>
  );
}

/** Exposed so the settings page can offer the same two choices. */
export const COMMIT_ACTIONS: { value: CommitActionPreference; key: string }[] = [
  { value: "commit", key: "commit-action-commit" },
  { value: "amend", key: "commit-action-amend" },
];
