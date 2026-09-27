/**
 * Overlay dialogs.
 *
 * Every destructive or ambiguous Git action is confirmed here first: creating,
 * renaming, and deleting refs, stashing, dropping a stash, merging, rebasing,
 * discarding changes, force pushing, publishing a branch, and recovering from a
 * failed merge or rebase. The WSL open dialog lives here too.
 *
 * Branch-name validation runs in the frontend so the confirm button reflects
 * the input as it is typed, and the backend re-validates by letting Git reject
 * anything invalid.
 */

import { useMemo, useState } from "react";

import { Icon } from "../../components/Icon";
import { Checkbox, DialogCard, TextInput } from "../../components/controls";
import * as ipc from "../../bridge/ipc";
import { integrationBlocked, localBranches, useStore, type Overlay } from "../../app/store";
import { validateBranchName } from "./branchName";
import { t, ta } from "../../i18n/strings";
import { WslOpenDialog } from "./WslOpenDialog";
import { preflightRebase } from "../repository/Toolbar";

export function Overlays({
  wslOpen,
  onWslOpenChange,
  onOpenPaths,
}: {
  wslOpen: boolean;
  onWslOpenChange: (open: boolean) => void;
  onOpenPaths: (paths: string[]) => Promise<void>;
}) {
  const overlay = useStore((state) => state.overlay);
  if (wslOpen) {
    return (
      <WslOpenDialog
        onClose={() => onWslOpenChange(false)}
        onOpen={async (distro, path) => {
          onWslOpenChange(false);
          await onOpenPaths([]);
          await useStore.getState().openTab(path, { kind: "wsl", distro });
        }}
      />
    );
  }
  if (overlay.kind === "none") {
    return null;
  }
  return <OverlayBody overlay={overlay} />;
}

function OverlayBody({ overlay }: { overlay: Overlay }) {
  switch (overlay.kind) {
    case "newBranch":
      return <NamedBranchDialog mode="newBranch" />;
    case "renameBranch":
      return <NamedBranchDialog mode="renameBranch" old={overlay.old} />;
    case "renameRemoteBranch":
      return (
        <NamedBranchDialog
          mode="renameRemoteBranch"
          old={overlay.old}
          remote={overlay.remote}
        />
      );
    case "stash":
      return <StashDialog />;
    case "stashDrop":
      return <StashDropDialog reference={overlay.reference} />;
    case "merge":
      return <MergeDialog noFf={overlay.noFf} />;
    case "rebase":
      return <RebaseDialog />;
    case "deleteRef":
      return <DeleteRefDialog name={overlay.name} isTag={overlay.isTag} />;
    case "deleteRemoteBranch":
      return (
        <DeleteRemoteBranchDialog remote={overlay.remote} branch={overlay.branch} />
      );
    case "forcePush":
      return <ForcePushDialog />;
    case "pushSetUpstream":
      return (
        <PushUpstreamDialog branch={overlay.branch} remote={overlay.remote} />
      );
    case "discard":
      return <DiscardDialog overlay={overlay} />;
    case "mergeConflict":
      return (
        <MergeConflictDialog source={overlay.source} detail={overlay.detail} />
      );
    case "mergeError":
      return <OperationErrorDialog label={overlay.label} detail={overlay.detail} />;
    case "rebaseConflict":
      return <RebaseConflictDialog detail={overlay.detail} />;
    case "rebaseError":
      return <OperationErrorDialog label={overlay.label} detail={overlay.detail} />;
    case "cliReport":
      return (
        <CliReportDialog report={overlay.report} />
      );
    default:
      return null;
  }
}

/** The repository the overlay acts on: the active tab. */
function useActiveRepoId(): number | null {
  const activeTabKey = useStore((state) => state.activeTabKey);
  const tabs = useStore((state) => state.tabs);
  const tab = tabs.find((entry) => entry.key === activeTabKey);
  return tab?.repoId ?? null;
}

// ===== Branch name dialogs =====

type NamedMode =
  | { mode: "newBranch" }
  | { mode: "renameBranch"; old: string }
  | { mode: "renameRemoteBranch"; old: string; remote: string };

function NamedBranchDialog(props: NamedMode) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const [name, setName] = useState(
    props.mode === "newBranch" ? "" : props.old,
  );
  const existing = useMemo(() => {
    if (!repo) {
      return [];
    }
    const names = localBranches(repo);
    if (repo.branch) {
      names.push(repo.branch);
    }
    return names;
  }, [repo]);

  // A remote branch name only has to satisfy ref syntax; the remote itself
  // rejects a name that already exists, so a duplicate is not checked here.
  const allow = props.mode === "renameBranch" ? props.old : undefined;
  const duplicateCheck = props.mode === "renameRemoteBranch" ? [] : existing;
  const error = validateBranchName(name, duplicateCheck, allow);
  const confirmEnabled = error === null;

  const title =
    props.mode === "newBranch"
      ? t(translate, "branch-new-title")
      : props.mode === "renameBranch"
        ? t(translate, "branch-rename-title")
        : t(translate, "rename-remote-branch-title");

  const icon =
    props.mode === "newBranch" ? (
      <Icon name="git-branch-plus" size={16} />
    ) : (
      <Icon name="pencil" size={16} />
    );

  const hint =
    props.mode === "newBranch"
      ? ta(translate, "branch-new-hint", { branch: repo?.branch || "HEAD" })
      : props.mode === "renameBranch"
        ? ta(translate, "branch-rename-hint", { branch: props.old })
        : ta(translate, "rename-remote-branch-hint", {
            remote: props.remote,
            branch: props.old,
          });

  const confirm = () => {
    if (!repoId || !confirmEnabled) {
      return;
    }
    closeOverlay();
    if (props.mode === "newBranch") {
      void runAction(repoId, { action: "createBranch", name });
    } else if (props.mode === "renameBranch") {
      void runAction(repoId, { action: "renameBranch", old: props.old, new: name });
    } else {
      void runAction(repoId, {
        action: "pushRenameRemote",
        remote: props.remote,
        old: props.old,
        new: name,
      });
    }
  };

  return (
    <DialogCard
      testId="branch-dialog"
      title={title}
      icon={icon}
      onBackdrop={closeOverlay}
      body={
        <>
          <label className="settings__label" htmlFor="branch-name-input">
            {t(translate, "branch-name-label")}
          </label>
          <TextInput
            value={name}
            onChange={setName}
            autoFocus
            monospace
            testId="branch-name-input"
            onSubmit={confirm}
            onEscape={closeOverlay}
          />
          <div className="settings__hint">{hint}</div>
          {error && error !== "empty" ? (
            <div className="status-conflict" data-testid="branch-name-error">
              {error === "exists"
                ? ta(translate, "branch-name-exists", { name })
                : t(translate, "branch-name-invalid")}
            </div>
          ) : null}
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="branch-dialog-cancel"
          >
            {t(translate, "dialog-cancel")}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={!confirmEnabled}
            onClick={confirm}
            data-testid="branch-dialog-confirm"
          >
            {t(translate, "dialog-confirm")}
          </button>
        </>
      }
    />
  );
}

// ===== Merge and rebase =====

function MergeDialog({ noFf }: { noFf: boolean }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const openOverlay = useStore((state) => state.openOverlay);
  const [source, setSource] = useState(() => (repo ? localBranches(repo)[0] ?? "" : ""));
  const [withNoFf, setWithNoFf] = useState(noFf);
  const [busy, setBusy] = useState(false);

  const branches = repo ? localBranches(repo) : [];

  const confirm = async () => {
    if (!repoId || !source) {
      return;
    }
    setBusy(true);
    try {
      // A merge that would be a no-op is refused with a clear message instead
      // of letting Git fail with "Already up to date".
      const probe = await ipc.probeMerge(repoId, source);
      if (probe.already_merged) {
        useStore
          .getState()
          .setMessage(repoId, t(useStore.getState().t, "merge-already-up-to-date"), false);
        setBusy(false);
        return;
      }
      if (integrationBlocked(probe)) {
        closeOverlay();
        if (probe.merge_head) {
          openOverlay({
            kind: "mergeConflict",
            source,
            detail: ta(useStore.getState().t, "merge-conflict-warning", { source }),
          });
        }
        setBusy(false);
        return;
      }
      closeOverlay();
      void runAction(repoId, { action: "merge", source, noFf: withNoFf });
    } catch (error) {
      const failure = ipc.describeError(error);
      useStore.getState().setMessage(repoId, failure.detail, false);
      setBusy(false);
    }
  };

  return (
    <DialogCard
      testId="merge-dialog"
      title={ta(translate, "merge-title", { branch: repo?.branch ?? "" })}
      icon={<Icon name="git-merge" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="row">
            <span className="muted">{t(translate, "merge-source-label")}</span>
            <select
              className="select__trigger"
              style={{ flex: 1 }}
              value={source}
              data-testid="merge-source"
              onChange={(event) => setSource(event.target.value)}
            >
              {branches.map((branch) => (
                <option key={branch} value={branch}>
                  {branch}
                </option>
              ))}
            </select>
          </div>
          <Checkbox
            checked={withNoFf}
            onChange={setWithNoFf}
            label={t(translate, "merge-no-ff-label")}
            testId="merge-no-ff"
          />
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="merge-dialog-cancel"
          >
            {t(translate, "dialog-cancel")}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={!source || busy}
            onClick={() => void confirm()}
            data-testid="merge-dialog-confirm"
          >
            {t(translate, "dialog-confirm")}
          </button>
        </>
      }
    />
  );
}

function RebaseDialog() {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const [source, setSource] = useState(() => (repo ? localBranches(repo)[0] ?? "" : ""));

  const branches = repo ? localBranches(repo) : [];

  const confirm = async () => {
    if (!repoId || !source) {
      return;
    }
    closeOverlay();
    if (repo) {
      await preflightRebase(repo, source);
    }
  };

  return (
    <DialogCard
      testId="rebase-dialog"
      title={ta(translate, "rebase-title", { branch: repo?.branch ?? "" })}
      icon={<Icon name="git-commit-horizontal" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="status-conflict">
            {ta(translate, "rebase-warning", { branch: repo?.branch ?? "" })}
          </div>
          <div className="row">
            <span className="muted">{t(translate, "merge-source-label")}</span>
            <select
              className="select__trigger"
              style={{ flex: 1 }}
              value={source}
              data-testid="rebase-source"
              onChange={(event) => setSource(event.target.value)}
            >
              {branches.map((branch) => (
                <option key={branch} value={branch}>
                  {branch}
                </option>
              ))}
            </select>
          </div>
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="rebase-dialog-cancel"
          >
            {t(translate, "dialog-cancel")}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={!source}
            onClick={() => void confirm()}
            data-testid="rebase-dialog-confirm"
          >
            {t(translate, "dialog-confirm")}
          </button>
        </>
      }
    />
  );
}

// ===== Stash =====

function StashDialog() {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const [message, setMessage] = useState("");

  return (
    <DialogCard
      testId="stash-dialog"
      title={t(translate, "stash-title")}
      icon={<Icon name="archive" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <>
          <label className="settings__label" htmlFor="stash-message">
            {t(translate, "stash-message-label")}
          </label>
          <TextInput
            value={message}
            onChange={setMessage}
            autoFocus
            testId="stash-message"
            onSubmit={() => {
              if (repoId) {
                closeOverlay();
                void runAction(repoId, { action: "stash", message });
              }
            }}
            onEscape={closeOverlay}
          />
          <div className="settings__hint">
            {ta(translate, "stash-hint", { count: repo?.stashableCount ?? 0 })}
          </div>
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="stash-dialog-cancel"
          >
            {t(translate, "dialog-cancel")}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            data-testid="stash-dialog-confirm"
            onClick={() => {
              if (repoId) {
                closeOverlay();
                void runAction(repoId, { action: "stash", message });
              }
            }}
          >
            {t(translate, "dialog-confirm")}
          </button>
        </>
      }
    />
  );
}

function StashDropDialog({ reference }: { reference: string }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);

  return (
    <DialogCard
      testId="stash-drop-dialog"
      title={t(translate, "stash-drop-title")}
      icon={<Icon name="trash-2" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <div className="muted">
          {ta(translate, "stash-drop-warning", { reference })}
        </div>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="stash-drop-cancel"
          >
            {t(translate, "dialog-cancel")}
          </button>
          <button
            type="button"
            className="tool-button tool-button--danger"
            data-testid="stash-drop-confirm"
            onClick={() => {
              if (repoId) {
                closeOverlay();
                void runAction(repoId, { action: "stashDrop", stashRef: reference });
              }
            }}
          >
            {t(translate, "dialog-confirm")}
          </button>
        </>
      }
    />
  );
}

// ===== Deletion =====

function DeleteRefDialog({ name, isTag }: { name: string; isTag: boolean }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);
  const [force, setForce] = useState(false);

  return (
    <DialogCard
      testId="delete-ref-dialog"
      title={t(translate, isTag ? "delete-tag-title" : "delete-branch-title")}
      icon={<Icon name="trash-2" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="muted">
            {ta(
              translate,
              isTag ? "delete-tag-warning" : "delete-branch-warning",
              { name },
            )}
          </div>
          {isTag ? null : (
            <Checkbox
              checked={force}
              onChange={setForce}
              label={t(translate, "delete-force-label")}
              testId="delete-force"
            />
          )}
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="delete-ref-cancel"
          >
            {t(translate, "dialog-cancel")}
          </button>
          <button
            type="button"
            className="tool-button tool-button--danger"
            data-testid="delete-ref-confirm"
            onClick={() => {
              if (!repoId) {
                return;
              }
              closeOverlay();
              void runAction(
                repoId,
                isTag
                  ? { action: "deleteTag", name }
                  : { action: "deleteBranch", name, force },
              );
            }}
          >
            {t(translate, "dialog-confirm")}
          </button>
        </>
      }
    />
  );
}

function DeleteRemoteBranchDialog({
  remote,
  branch,
}: {
  remote: string;
  branch: string;
}) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);

  return (
    <DialogCard
      testId="delete-remote-dialog"
      title={t(translate, "delete-remote-branch-title")}
      icon={<Icon name="trash-2" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <div className="muted">
          {ta(translate, "delete-remote-branch-warning", { remote, branch })}
        </div>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="delete-remote-cancel"
          >
            {t(translate, "dialog-cancel")}
          </button>
          <button
            type="button"
            className="tool-button tool-button--danger"
            data-testid="delete-remote-confirm"
            onClick={() => {
              if (repoId) {
                closeOverlay();
                void runAction(repoId, {
                  action: "pushDeleteRemote",
                  remote,
                  branch,
                });
              }
            }}
          >
            {t(translate, "dialog-confirm")}
          </button>
        </>
      }
    />
  );
}

// ===== Push confirmations =====

function ForcePushDialog() {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);

  return (
    <DialogCard
      testId="force-push-dialog"
      title={
        <>
          <Icon name="triangle-alert" size={16} />{" "}
          {t(translate, "push-force-title")}
        </>
      }
      onBackdrop={closeOverlay}
      body={<div className="muted">{t(translate, "push-force-warning")}</div>}
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="force-push-cancel"
          >
            {t(translate, "push-force-cancel")}
          </button>
          <button
            type="button"
            className="tool-button tool-button--danger"
            data-testid="force-push-confirm"
            onClick={() => {
              if (repoId) {
                closeOverlay();
                void runAction(repoId, { action: "pushForce" });
              }
            }}
          >
            {t(translate, "push-force-confirm")}
          </button>
        </>
      }
    />
  );
}

function PushUpstreamDialog({
  branch,
  remote,
}: {
  branch: string;
  remote: string;
}) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);

  return (
    <DialogCard
      testId="push-upstream-dialog"
      title={
        <>
          <Icon name="chevron-down" size={16} />{" "}
          {t(translate, "push-upstream-title")}
        </>
      }
      onBackdrop={closeOverlay}
      body={
        <div className="muted">
          {ta(translate, "push-upstream-warning", { branch, remote })}
        </div>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="push-upstream-cancel"
          >
            {t(translate, "push-upstream-cancel")}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            data-testid="push-upstream-confirm"
            onClick={() => {
              if (repoId) {
                closeOverlay();
                void runAction(repoId, {
                  action: "pushSetUpstream",
                  remote,
                  branch,
                });
              }
            }}
          >
            {t(translate, "push-upstream-confirm")}
          </button>
        </>
      }
    />
  );
}

// ===== Discard =====

function DiscardDialog({
  overlay,
}: {
  overlay: Extract<Overlay, { kind: "discard" }>;
}) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const repo = useStore((state) => (repoId ? state.repos[repoId] : undefined));
  const closeOverlay = useStore((state) => state.closeOverlay);
  const [busy, setBusy] = useState(false);

  const files = useMemo(() => {
    if (!repo) {
      return [];
    }
    const staged = overlay.scope.staged;
    return repo.files.filter(
      (file) => (staged ? file.index !== " " : file.worktree !== " "),
    );
  }, [repo, overlay.scope.staged]);

  const warning =
    overlay.scope.all && files.length === 1
      ? t(translate, "discard-file-warning")
      : overlay.trackedCount > 0
        ? t(translate, "discard-all-warning")
        : t(translate, "discard-untracked-file-warning");

  return (
    <DialogCard
      testId="discard-dialog"
      title={t(translate, "discard-title")}
      icon={<Icon name="undo" size={16} />}
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="muted">{warning}</div>
          {files.length > 0 && files.length <= 12 ? (
            <ul className="stack stack--tight" style={{ margin: 0, paddingLeft: 18 }}>
              {files.map((file) => (
                <li key={file.path} className="mono">
                  {file.path}
                </li>
              ))}
            </ul>
          ) : null}
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="discard-cancel"
          >
            {t(translate, "discard-cancel")}
          </button>
          <button
            type="button"
            className="tool-button tool-button--danger"
            disabled={busy}
            data-testid="discard-confirm"
            onClick={async () => {
              if (!repoId) {
                return;
              }
              setBusy(true);
              try {
                await ipc.workingTreeOperation(
                  repoId,
                  "discard",
                  files,
                  overlay.scope.all,
                );
                useStore.getState().setBusy(repoId, true);
                closeOverlay();
              } catch (error) {
                useStore
                  .getState()
                  .setMessage(repoId, ipc.describeError(error).detail, false);
                setBusy(false);
              }
            }}
          >
            {t(translate, "discard-confirm")}
          </button>
        </>
      }
    />
  );
}

// ===== Failure recovery =====

function MergeConflictDialog({
  source,
  detail,
}: {
  source: string;
  detail: string;
}) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);

  return (
    <DialogCard
      testId="merge-conflict-dialog"
      title={
        <>
          <Icon name="triangle-alert" size={16} />{" "}
          {t(translate, "merge-conflict-title")}
        </>
      }
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="muted">
            {ta(translate, "merge-conflict-warning", { source })}
          </div>
          <pre
            className="status-conflict"
            style={{ maxHeight: 180, overflow: "auto", margin: 0, fontSize: "0.7em" }}
            data-testid="merge-conflict-detail"
          >
            {detail}
          </pre>
        </>
      }
      footer={
        <button
          type="button"
          className="tool-button tool-button--danger"
          data-testid="merge-abort"
          onClick={() => {
            if (repoId) {
              closeOverlay();
              void runAction(repoId, { action: "abortMerge" });
            }
          }}
        >
          {t(translate, "merge-abort")}
        </button>
      }
    />
  );
}

function RebaseConflictDialog({ detail }: { detail: string }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);

  return (
    <DialogCard
      testId="rebase-conflict-dialog"
      title={
        <>
          <Icon name="triangle-alert" size={16} />{" "}
          {t(translate, "rebase-conflict-title")}
        </>
      }
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="muted">{t(translate, "rebase-conflict-warning")}</div>
          <pre
            className="status-conflict"
            style={{ maxHeight: 180, overflow: "auto", margin: 0, fontSize: "0.7em" }}
            data-testid="rebase-conflict-detail"
          >
            {detail}
          </pre>
        </>
      }
      footer={
        <button
          type="button"
          className="tool-button tool-button--danger"
          data-testid="rebase-abort"
          onClick={() => {
            if (repoId) {
              closeOverlay();
              void runAction(repoId, { action: "abortRebase" });
            }
          }}
        >
          {t(translate, "rebase-abort")}
        </button>
      }
    />
  );
}

function OperationErrorDialog({
  label,
  detail,
}: {
  label: string;
  detail: string;
}) {
  const translate = useStore((state) => state.t);
  const closeOverlay = useStore((state) => state.closeOverlay);

  return (
    <DialogCard
      testId="operation-error-dialog"
      title={
        <>
          <Icon name="triangle-alert" size={16} />{" "}
          {ta(translate, "command-failed", { label, error: "" }).split(":")[0]}
        </>
      }
      onBackdrop={closeOverlay}
      body={
        <pre
          className="status-conflict"
          style={{ maxHeight: 220, overflow: "auto", margin: 0, fontSize: "0.7em" }}
          data-testid="operation-error-detail"
        >
          {detail}
        </pre>
      }
      footer={
        <button
          type="button"
          className="tool-button tool-button--primary"
          onClick={closeOverlay}
          data-testid="operation-error-close"
        >
          {t(translate, "rebase-error-close")}
        </button>
      }
    />
  );
}

// ===== CLI installer report =====

function CliReportDialog({
  report,
}: {
  report: { entries: { path: string; outcome: string }[]; fallbackBinary: boolean };
}) {
  const translate = useStore((state) => state.t);
  const closeOverlay = useStore((state) => state.closeOverlay);
  const storePaths = useStore((state) => state.storePaths);

  const title = t(translate, "cli-dialog-title");
  const verb = (outcome: string) => {
    switch (outcome) {
      case "updated":
        return t(translate, "cli-install-updated");
      case "unchanged":
        return t(translate, "cli-install-unchanged");
      case "notInstalled":
        return t(translate, "cli-remove-notinstalled");
      default:
        return t(translate, "cli-install-failed");
    }
  };

  return (
    <DialogCard
      testId="cli-report-dialog"
      title={title}
      onBackdrop={closeOverlay}
      width={480}
      body={
        <>
          {report.entries.length === 0 ? (
            <div className="muted">
              {t(translate, "cli-install-none")}
            </div>
          ) : (
            <ul className="stack stack--tight" style={{ margin: 0, paddingLeft: 18 }}>
              {report.entries.map((entry) => (
                <li key={entry.path}>
                  {verb(entry.outcome)} <span className="mono">{entry.path}</span>
                </li>
              ))}
            </ul>
          )}
          {report.fallbackBinary ? (
            <div className="status-mod">{t(translate, "cli-binary-fallback")}</div>
          ) : null}
          <div className="muted">{t(translate, "cli-install-hint")}</div>
          {storePaths.length ? (
            <div className="settings__hint">
              {t(translate, "settings-store-location")}:{" "}
              <span className="mono">{storePaths.join(", ")}</span>
            </div>
          ) : null}
        </>
      }
      footer={
        <button
          type="button"
          className="tool-button tool-button--primary"
          onClick={closeOverlay}
          data-testid="cli-report-close"
        >
          {t(translate, "dialog-cancel")}
        </button>
      }
    >
    </DialogCard>
  );
}

export { WslOpenDialog };
export type { NamedMode };
