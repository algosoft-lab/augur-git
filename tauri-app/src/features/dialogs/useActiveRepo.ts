import { useStore } from '../../app/store';

/** The repository the overlay acts on: the active tab. */
export function useActiveRepoId(): number | null {
  const activeTabKey = useStore((state) => state.activeTabKey);
  const tabs = useStore((state) => state.tabs);
  const tab = tabs.find((entry) => entry.key === activeTabKey);
  return tab && tab.repoId !== null ? tab.repoId : null;
}
