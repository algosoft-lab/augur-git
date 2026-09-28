import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { Icon } from "../../components/Icon";
import { TextInput } from "../../components/controls";
import type { CompareRevision } from "../../bridge/types";
import { useStore } from "../../app/store";
import { t } from "../../i18n/strings";
import { filterRevisions, isRevisionUnavailable } from "./revisions";

export interface Endpoint {
  manualInput: string;
  selected: CompareRevision | null;
}

interface PopupPosition {
  left: number;
  top?: number;
  bottom?: number;
  width: number;
  maxHeight: number;
}

/** Turn typed text into a revision, accepting a 7 to 64 digit object id. */
function fromManualInput(input: string): CompareRevision | null {
  const text = input.trim();
  if (text.length < 7 || text.length > 64) {
    return null;
  }
  if (!/^[0-9a-fA-F]+$/.test(text)) {
    return null;
  }
  return { name: text, full_name: text, kind: "commit" };
}

export function endpointRevision(endpoint: Endpoint): CompareRevision | null {
  return endpoint.selected ?? fromManualInput(endpoint.manualInput);
}

/** A searchable selector for named refs and manual commit SHA entry. */
export function RevisionPicker({
  label,
  endpoint,
  options,
  onChange,
}: {
  label: string;
  endpoint: Endpoint;
  options: CompareRevision[];
  onChange: (next: Endpoint) => void;
}) {
  const translate = useStore((state) => state.t);
  const [open, setOpen] = useState(false);
  const [highlighted, setHighlighted] = useState(0);
  const [query, setQuery] = useState("");
  const [manualOnly, setManualOnly] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const anchorRef = useRef<HTMLDivElement>(null);
  const optionsRef = useRef<HTMLDivElement>(null);
  const [popupPosition, setPopupPosition] = useState<PopupPosition | null>(null);
  const filtered = useMemo(() => filterRevisions(options, query), [options, query]);
  const unavailable = isRevisionUnavailable(endpoint.selected, options);

  useLayoutEffect(() => {
    if (!open) {
      setPopupPosition(null);
      return;
    }

    const updatePosition = () => {
      const anchor = anchorRef.current;
      if (!anchor) {
        return;
      }
      const rect = anchor.getBoundingClientRect();
      const edge = 8;
      const width = Math.min(
        Math.max(rect.width, Math.min(280, window.innerWidth - edge * 2)),
        window.innerWidth - edge * 2,
      );
      const left = Math.max(edge, Math.min(rect.left, window.innerWidth - width - edge));
      const spaceBelow = window.innerHeight - rect.bottom - edge;
      const spaceAbove = rect.top - edge;
      const openAbove = spaceBelow < 180 && spaceAbove > spaceBelow;
      const maxHeight = Math.min(280, Math.max(0, openAbove ? spaceAbove : spaceBelow));
      setPopupPosition({
        left,
        width,
        maxHeight,
        ...(openAbove
          ? { bottom: window.innerHeight - rect.top + 2 }
          : { top: rect.bottom + 2 }),
      });
    };

    updatePosition();
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    const observer = new ResizeObserver(updatePosition);
    if (anchorRef.current) {
      observer.observe(anchorRef.current);
    }
    return () => {
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
      observer.disconnect();
    };
  }, [open]);

  useEffect(() => {
    if (!open) {
      return;
    }
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (
        !rootRef.current?.contains(target) &&
        !optionsRef.current?.contains(target)
      ) {
        setOpen(false);
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [open]);

  useEffect(() => {
    setHighlighted(0);
  }, [query]);

  const closePopup = () => {
    setOpen(false);
    setQuery("");
  };

  const chooseRevision = (choice: CompareRevision) => {
    onChange({ manualInput: "", selected: choice });
    closePopup();
  };

  const toggleManual = (enabled: boolean) => {
    setManualOnly(enabled);
    closePopup();
    onChange({ manualInput: "", selected: null });
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") {
      closePopup();
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      if (manualOnly) {
        return;
      }
      event.preventDefault();
      if (!open) {
        setQuery("");
      }
      setOpen(true);
      setHighlighted((current) => {
        if (filtered.length === 0) {
          return 0;
        }
        const step = event.key === "ArrowDown" ? 1 : -1;
        return (current + step + filtered.length) % filtered.length;
      });
      return;
    }
    if (event.key === "Enter" && !manualOnly) {
      const choice = filtered[highlighted];
      if (choice) {
        event.preventDefault();
        chooseRevision(choice);
      }
    }
  };

  const groupOrder = query.trim()
    ? [...new Set(filtered.map((option) => option.kind))]
    : ["local", "remote", "tag"] as const;
  const groups = groupOrder.map((kind) => {
    const group = filtered.filter((option) => option.kind === kind);
    if (group.length === 0) {
      return null;
    }
    return (
      <div key={kind}>
        <div className="compare__picker-section">
          {t(
            translate,
            kind === "local"
              ? "branch-compare-branches"
              : kind === "remote"
                ? "branch-compare-remote"
                : "branch-compare-tags",
          )}
        </div>
        {group.map((option) => (
          <button
            key={option.full_name}
            type="button"
            role="option"
            aria-selected={filtered[highlighted] === option}
            className={`compare__picker-option${
              filtered[highlighted] === option ? " is-highlighted" : ""
            }`}
            data-testid={`compare-option-${option.kind}-${option.full_name}`}
            onClick={() => chooseRevision(option)}
          >
            <Icon name="git-branch" size={11} />
            {`${t(translate, `branch-compare-${kind}`)} · ${option.name}`}
          </button>
        ))}
      </div>
    );
  });

  return (
    <div className="compare__picker" ref={rootRef} onKeyDown={onKeyDown}>
      <span className="compare__picker-label">{label}</span>
      <div
        className="compare__picker-input"
        ref={anchorRef}
        onClick={(event) => {
          if (event.target instanceof HTMLInputElement && !manualOnly && !open) {
            setQuery("");
            setOpen(true);
          }
        }}
      >
        <TextInput
          value={
            manualOnly
              ? endpoint.manualInput
              : open
                ? query
                : endpoint.selected?.name ?? ""
          }
          size="small"
          monospace
          testId={`compare-input-${label}`}
          onChange={(value) => {
            if (manualOnly) {
              const revision = fromManualInput(value);
              onChange({ manualInput: value, selected: revision });
              setOpen(value.trim().length > 0 && revision === null);
            } else {
              setQuery(value);
              setOpen(true);
            }
          }}
          onSubmit={() => {
            if (!manualOnly) {
              setOpen(true);
            }
          }}
          placeholder={t(
            translate,
            manualOnly
              ? "branch-compare-sha-placeholder"
              : "branch-compare-revision-placeholder",
          )}
        />
        <label
          className="switch"
          data-testid={`compare-manual-${label}`}
          title={t(translate, "branch-compare-manual-input")}
        >
          <input
            type="checkbox"
            checked={manualOnly}
            onChange={(event) => toggleManual(event.target.checked)}
          />
          <span>{t(translate, "branch-compare-manual-input")}</span>
        </label>
        {!manualOnly ? (
          <button
            type="button"
            className="tool-button tool-button--compact"
            data-testid={`compare-toggle-${label}`}
            aria-label={t(translate, "branch-compare-open-picker")}
            aria-expanded={open}
            onClick={() => {
              if (open) {
                closePopup();
              } else {
                setQuery("");
                setOpen(true);
              }
            }}
          >
            <Icon name="chevron-down" size={11} />
          </button>
        ) : null}
        {open && popupPosition
          ? createPortal(
              <div
                ref={optionsRef}
                className="compare__picker-options"
                data-testid={`compare-options-${label}`}
                style={popupPosition}
                role={manualOnly ? "alert" : "listbox"}
              >
                {manualOnly ? (
                  <div
                    className="compare__picker-error"
                    data-testid={`compare-manual-error-${label}`}
                  >
                    {t(translate, "branch-compare-invalid-sha")}
                  </div>
                ) : filtered.length === 0 ? (
                  <div className="compare__picker-section">
                    {t(translate, "branch-compare-no-matches")}
                  </div>
                ) : null}
                {!manualOnly ? groups : null}
                {unavailable ? (
                  <div
                    className="compare__picker-error"
                    data-testid={`compare-unavailable-${label}`}
                  >
                    {t(translate, "branch-compare-revision-unavailable")}
                  </div>
                ) : null}
              </div>,
              document.body,
            )
          : null}
      </div>
    </div>
  );
}
