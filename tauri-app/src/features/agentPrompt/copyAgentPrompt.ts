
import * as ipc from '../../bridge/ipc';
import type { AgentPromptRequest } from '../../bridge/types';
import { useStore } from '../../app/store';
import { copyText } from '../../platform';

export async function copyAgentPrompt(
  repoId: number,
  request: AgentPromptRequest
): Promise<boolean> {
  const initial = useStore.getState();
  if (!initial.repos[repoId] || initial.repos[repoId].busy) {
    return false;
  }

  let prompt: string;
  try {
    prompt = await ipc.generateAgentPrompt(repoId, request);
  } catch (error) {
    useStore.getState().reportError(repoId, error);
    return false;
  }

  try {
    await copyText(prompt);
  } catch {
    useStore
      .getState()
      .setMessage(repoId, useStore.getState().t('agent-prompt-copy-failed'), false);
    return false;
  }

  useStore.getState().setMessage(repoId, useStore.getState().t('agent-prompt-copied'), true);
  return true;
}
