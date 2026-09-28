/**
 * One repository tab: the three-column layout with its splitters.
 *
 * The sidebar and right panel are fixed-width columns and the bottom panel has
 * an optional fixed height; all three are persisted so a restart restores the
 * shape the user left behind. Dragging clamps to the same bounds the reference
 * application uses.
 */

import { useEffect, useRef, useState } from "react";

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
const MIN_CENTER_WIDTH = 280;
const TOOLBAR_HEIGHT = 32;
/** Height of the horizontal resize handle between the graph and the diff. */
const DIFF_HANDLE = 3;
/** Viewport height used when the real one is not known yet. */
const FALLBACK_SIZE = { width: 1280, height: 800 };

export function RepoTab({ repo }: { repo: RepoState }) {
  const translate = useStore((state) => state.t);
  const layout = useStore((state) => state.workspace.layout);
  const historyScope = useStore((state) => state.config.view.graph_history);
  const previewLayout = useStore((state) => state.previewLayout);
  const persistLayout = useStore((state) => state.persistLayout);
  const setLogScope = useStore((state) => state.setLogScope);
  const repoRef = useRef<HTMLDivElement>(null);
  const sidebarDragStart = useRef(layout.sidebar_width);
  const rightPanelDragStart = useRef(layout.right_panel_width);
  const diffDragStart = useRef(layout.diff_height ?? 320);
  const [size, setSize] = useState(FALLBACK_SIZE);

  useEffect(() => {
    const element = repoRef.current;
    if (!element) {
      return;
    }
    const observer = new ResizeObserver(() => {
      setSize({ width: element.clientWidth, height: element.clientHeight });
    });
    observer.observe(element);
    setSize({ width: element.clientWidth, height: element.clientHeight });
    return () => observer.disconnect();
  }, []);

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
      size.height - TOOLBAR_HEIGHT - MIN_COMMIT_HEIGHT - DIFF_HANDLE,
    ),
  );
  const diffHeight =
    layout.diff_height === null
      ? null
      : Math.min(maxDiffHeight, Math.max(MIN_DIFF_HEIGHT, layout.diff_height));
  const rightPanelPreferred = clamp(
    layout.right_panel_width,
    MIN_RIGHT_PANEL_WIDTH,
    Math.max(
      MIN_RIGHT_PANEL_WIDTH,
      Math.min(
        MAX_RIGHT_PANEL_WIDTH,
        size.width - MIN_SIDEBAR_WIDTH - MIN_CENTER_WIDTH,
      ),
    ),
  );
  const sidebarMax = Math.max(
    MIN_SIDEBAR_WIDTH,
    Math.min(
      MAX_SIDEBAR_WIDTH,
      size.width - rightPanelPreferred - MIN_CENTER_WIDTH,
    ),
  );
  const sidebarWidth = clamp(layout.sidebar_width, MIN_SIDEBAR_WIDTH, sidebarMax);
  const rightPanelMax = Math.max(
    MIN_RIGHT_PANEL_WIDTH,
    Math.min(
      MAX_RIGHT_PANEL_WIDTH,
      size.width - sidebarWidth - MIN_CENTER_WIDTH,
    ),
  );
  const rightPanelWidth = clamp(
    rightPanelPreferred,
    MIN_RIGHT_PANEL_WIDTH,
    rightPanelMax,
  );

  return (
    <div className="repo" ref={repoRef} data-testid={`repo-${repo.id}`}>
      <div
        className="repo__sidebar"
        style={{ width: sidebarWidth }}
        data-testid="repo-sidebar"
      >
        <Sidebar repo={repo} refs={repo.refs} />
        <Splitter
          orientation="vertical"
          label={t(translate, "sidebar-repo")}
          testId="sidebar-splitter"
          onDragStart={() => {
            sidebarDragStart.current = sidebarWidth;
          }}
          onDrag={(delta) => {
            previewLayout({
              sidebar_width: clamp(
                sidebarDragStart.current + delta,
                MIN_SIDEBAR_WIDTH,
                Math.max(
                  MIN_SIDEBAR_WIDTH,
                  Math.min(
                    MAX_SIDEBAR_WIDTH,
                    size.width - rightPanelWidth - MIN_CENTER_WIDTH,
                  ),
                ),
              ),
            });
          }}
          onDragEnd={() => void persistLayout()}
        />
      </div>

      <div className="repo__center">
        <Toolbar repo={repo} />
        <GraphView repo={repo} />
        <Splitter
          orientation="horizontal"
          label="resize diff"
          testId="diff-splitter"
          onDragStart={() => {
            diffDragStart.current = diffHeight ?? 320;
          }}
          onDrag={(delta) => {
            const next = diffDragStart.current - delta;
            previewLayout({
              diff_height: clamp(next, MIN_DIFF_HEIGHT, maxDiffHeight),
            });
          }}
          onDragEnd={() => void persistLayout()}
        />
        <BottomPanel
          repo={repo}
          height={diffHeight}
          onFileListRatioChange={(ratio) => {
            previewLayout({ file_list_ratio: ratio });
          }}
          onFileListRatioChangeEnd={() => void persistLayout()}
        />
      </div>

      <div
        className="repo__right"
        style={{ width: rightPanelWidth }}
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
          onDragStart={() => {
            rightPanelDragStart.current = rightPanelWidth;
          }}
          onDrag={(delta) => {
            previewLayout({
              right_panel_width: clamp(
                rightPanelDragStart.current - delta,
                MIN_RIGHT_PANEL_WIDTH,
                Math.max(
                  MIN_RIGHT_PANEL_WIDTH,
                  Math.min(
                    MAX_RIGHT_PANEL_WIDTH,
                    size.width - sidebarWidth - MIN_CENTER_WIDTH,
                  ),
                ),
              ),
            });
          }}
          onDragEnd={() => void persistLayout()}
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
