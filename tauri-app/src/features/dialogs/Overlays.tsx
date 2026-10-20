/**
 * The overlay router.
 *
 * Every destructive or ambiguous Git action is confirmed first. The dialogs
 * themselves are grouped by what they act on, in the sibling modules; this file
 * only decides which one the current overlay state calls for, and hosts the WSL
 * open dialog because that one is a mode of the window rather than an action on
 * a repository.
 */

import { useStore, type Overlay } from '../../app/store';
import { WslOpenDialog } from './WslOpenDialog';
import {
  DeleteRefDialog,
  DeleteRemoteBranchDialog,
  DeleteRemoteTagDialog,
  NamedBranchDialog,
  RenameRemoteTagDialog
} from './branchDialogs';
import {
  MergeConflictDialog,
  MergeDialog,
  RebaseConflictDialog,
  RebaseDialog,
  StashPopConflictDialog
} from './integrationDialogs';
import { DiscardDialog, StashDialog, StashDropDialog } from './workingTreeDialogs';
import { GitIdentityDialog } from './GitIdentityDialog';
import { ForcePushDialog, PushUpstreamDialog } from './pushDialogs';
import { ResetDialog } from './resetDialog';
import { OperationErrorDialog } from './reportDialogs';
import { ManageRemotesDialog } from './remoteDialogs';
import { LfsDialog } from './LfsDialog';
import { LfsCloneDialog } from './LfsCloneDialog';

export function Overlays({
  wslOpen,
  onWslOpenChange,
  onOpenPaths
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
          await useStore.getState().openTab(path, { kind: 'wsl', distro });
        }}
      />
    );
  }
  if (overlay.kind === 'none') {
    return null;
  }
  return <OverlayBody overlay={overlay} />;
}

function OverlayBody({ overlay }: { overlay: Overlay }) {
  switch (overlay.kind) {
    case 'newBranch':
      return <NamedBranchDialog mode="newBranch" />;
    case 'renameBranch':
      return <NamedBranchDialog mode="renameBranch" old={overlay.old} />;
    case 'renameRemoteBranch':
      return (
        <NamedBranchDialog mode="renameRemoteBranch" old={overlay.old} remote={overlay.remote} />
      );
    case 'stash':
      return <StashDialog />;
    case 'stashDrop':
      return <StashDropDialog reference={overlay.reference} />;
    case 'merge':
      return <MergeDialog noFf={overlay.noFf} />;
    case 'rebase':
      return <RebaseDialog />;
    case 'reset':
      return <ResetDialog />;
    case 'deleteRef':
      return <DeleteRefDialog name={overlay.name} isTag={overlay.isTag} />;
    case 'deleteRemoteBranch':
      return <DeleteRemoteBranchDialog remote={overlay.remote} branch={overlay.branch} />;
    case 'deleteRemoteTag':
      return <DeleteRemoteTagDialog name={overlay.name} />;
    case 'renameRemoteTag':
      return <RenameRemoteTagDialog old={overlay.old} />;
    case 'forcePush':
      return <ForcePushDialog />;
    case 'pushSetUpstream':
      return <PushUpstreamDialog branch={overlay.branch} remote={overlay.remote} />;
    case 'manageRemotes':
      return (
        <ManageRemotesDialog
          // The dialog is mounted once per focus, so opening it on another
          // remote starts from that remote rather than from the last one.
          key={overlay.focus ? `${overlay.focus.remote}:${overlay.focus.panel}` : 'manage'}
          focus={overlay.focus}
        />
      );
    case 'lfs':
      return <LfsDialog repoId={overlay.repoId} />;
    case 'lfsClone':
      return <LfsCloneDialog />;
    case 'discard':
      return <DiscardDialog overlay={overlay} />;
    case 'mergeConflict':
      return <MergeConflictDialog source={overlay.source} detail={overlay.detail} />;
    case 'mergeError':
      return (
        <OperationErrorDialog
          label={overlay.label}
          detail={overlay.detail}
          titleKey="merge-error-title"
        />
      );
    case 'rebaseConflict':
      return (
        <RebaseConflictDialog
          detail={overlay.detail}
          source={overlay.source}
          label={overlay.label}
        />
      );
    case 'rebaseError':
      return (
        <OperationErrorDialog
          label={overlay.label}
          detail={overlay.detail}
          titleKey="merge-error-title"
        />
      );
    case 'stashPopConflict':
      return <StashPopConflictDialog detail={overlay.detail} />;
    case 'gitIdentity':
      return <GitIdentityDialog overlay={overlay} />;
    default:
      return null;
  }
}
