import { writeText } from '@tauri-apps/plugin-clipboard-manager';

import * as ipc from '../../bridge/ipc';
import type { AgentPromptRequest } from '../../bridge/types';
import { useStore } from '../../app/store';

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
    await writeText(prompt);
  } catch {
    useStore
      .getState()
      .setMessage(repoId, useStore.getState().t('agent-prompt-copy-failed'), false);
    return false;
  }

  useStore.getState().setMessage(repoId, useStore.getState().t('agent-prompt-copied'), true);
  return true;
}
