/**
 * One repository tab: the three-column layout with its splitters.
 *
 * The sidebar and right panel are fixed-width columns and the bottom panel has
 * an optional fixed height; all three are persisted so a restart restores the
 * shape the user left behind. Dragging clamps to the same bounds the reference
 * application uses.
 */

import { useEffect, useRef } from "react";

import { Splitter } from "../../components/controls";
import {
  MAX_DIFF_HEIGHT,
  MAX_RIGHT_PANEL_WIDTH,
  MAX_SIDEBAR_WIDTH,
  MIN_DIFF_HEIGHT,
  MIN_RIGHT_PANEL_WIDTH,
  MIN_SIDEBAR_WIDTH,
} from "./bounds";
import { useStore, type RepoState } from "../../app/store";
import { Sidebar } from "./Sidebar";
import { Toolbar } from "./Toolbar";
import { ChangesPanel } from "./ChangesPanel";
import { CommitPanel } from "./CommitPanel";
import { GraphView } from "../graph/GraphView";
import { BottomPanel } from "../diff/BottomPanel";
import { t } from "../../i18n/strings";

/** Minimum height of the commit editor, matching the reference application. */
const MIN_COMMIT_HEIGHT = 120;
/** Height of the horizontal resize handle between the graph and the diff. */
const DIFF_HANDLE = 3;
/** Viewport height used when the real one is not known yet. */
const FALLBACK_VIEWPORT_HEIGHT = 800;

export function RepoTab({ repo }: { repo: RepoState }) {
  const translate = useStore((state) => state.t);
  const layout = useStore((state) => state.workspace.layout);
  const historyScope = useStore((state) => state.config.view.graph_history);
  const updateLayout = useStore((state) => state.updateLayout);
  const setLogScope = useStore((state) => state.setLogScope);
  const viewportHeight = useViewportHeight();

  // The history scope is sent once a repository is ready, and again whenever
  // the preference changes. The tracked upstream is part of the scope, so a
  // repository that only learns its upstream after the first status is covered
  // by the key below.
  const scopeKey = `${historyScope}:${repo.upstream ?? ""}`;
  const lastScope = useRef<string | null>(null);
  useEffect(() => {
    if (repo.status !== "ready") {
      return;
    }
    if (lastScope.current === scopeKey) {
      return;
    }
    lastScope.current = scopeKey;
    void setLogScope(repo.id);
  }, [repo.id, repo.status, scopeKey, setLogScope]);

  const maxDiffHeight = Math.max(
    MIN_DIFF_HEIGHT,
    Math.min(
      MAX_DIFF_HEIGHT,
      viewportHeight - MIN_COMMIT_HEIGHT - DIFF_HANDLE,
    ),
  );
  const diffHeight =
    layout.diff_height === null
      ? null
      : Math.min(maxDiffHeight, Math.max(MIN_DIFF_HEIGHT, layout.diff_height));

  return (
    <div className="repo" data-testid={`repo-${repo.id}`}>
      <div
        className="repo__sidebar"
        style={{ width: layout.sidebar_width }}
        data-testid="repo-sidebar"
      >
        <Sidebar repo={repo} refs={repo.refs} />
        <Splitter
          orientation="vertical"
          label={t(translate, "sidebar-repo")}
          testId="sidebar-splitter"
          onDrag={(delta) => {
            void updateLayout({
              sidebar_width: clamp(
                layout.sidebar_width + delta,
                MIN_SIDEBAR_WIDTH,
                MAX_SIDEBAR_WIDTH,
              ),
            });
          }}
        />
      </div>

      <div className="repo__center">
        <Toolbar repo={repo} />
        <GraphView repo={repo} />
        <Splitter
          orientation="horizontal"
          label="resize diff"
          testId="diff-splitter"
          onDrag={(delta) => {
            const next = diffHeight === null ? 320 : diffHeight - delta;
            void updateLayout({
              diff_height: clamp(next, MIN_DIFF_HEIGHT, maxDiffHeight),
            });
          }}
        />
        <BottomPanel
          repo={repo}
          height={diffHeight}
          onFileListRatioChange={(ratio) => {
            void updateLayout({ file_list_ratio: ratio });
          }}
        />
      </div>

      <div
        className="repo__right"
        style={{ width: layout.right_panel_width }}
        data-testid="repo-right"
      >
        <div className="right-panel">
          <CommitPanel repo={repo} />
          <div
            style={{ height: 1, flex: "0 0 auto", background: "var(--border)" }}
          />
          <ChangesPanel repo={repo} />
        </div>
        <Splitter
          orientation="vertical"
          label={t(translate, "changes-title")}
          testId="right-splitter"
          onDrag={(delta) => {
            void updateLayout({
              right_panel_width: clamp(
                layout.right_panel_width - delta,
                MIN_RIGHT_PANEL_WIDTH,
                MAX_RIGHT_PANEL_WIDTH,
              ),
            });
          }}
        />
      </div>
    </div>
  );
}

function clamp(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) {
    return min;
  }
  return Math.min(max, Math.max(min, value));
}

/** Track the viewport height so the diff height can be clamped to it. */
function useViewportHeight(): number {
  const ref = useRef(FALLBACK_VIEWPORT_HEIGHT);
  useEffect(() => {
    const onResize = () => {
      ref.current = window.innerHeight;
    };
    onResize();
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return typeof window === "undefined" ? FALLBACK_VIEWPORT_HEIGHT : window.innerHeight;
}
