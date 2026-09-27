/**
 * The Windows-only dialog for opening a repository inside a WSL distribution.
 *
 * The distribution list comes from `wsl -l -q`, and the path is validated with
 * the same probe the worker runs on open, so an invalid combination is reported
 * here instead of producing a broken tab. A pasted `\\wsl$` path is split into
 * its distribution and Linux path.
 */

import { useCallback, useEffect, useState } from "react";

import { Icon } from "../../components/Icon";
import { Checkbox, DialogCard, Spinner, TextInput } from "../../components/controls";
import * as ipc from "../../bridge/ipc";
import { useStore } from "../../app/store";
import { t, ta } from "../../i18n/strings";

type Validation =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "ok" }
  | { kind: "failed"; detail: string };

/** Split a pasted `\\wsl$\Distro\path` into its two halves. */
export function splitUncPath(
  input: string,
): { distro: string; path: string } | null {
  const match = /^\\\\wsl\$\\([^\\]+)\\(.*)$/.exec(input.trim());
  if (!match) {
    return null;
  }
  const distro = match[1] ?? "";
  const rest = match[2] ?? "";
  return distro && rest ? { distro, path: `/${rest.replace(/\\/g, "/")}` } : null;
}

/** Whether a value looks like an absolute Linux path. */
export function isAbsoluteLinuxPath(value: string): boolean {
  return value.startsWith("/");
}

export function WslOpenDialog({
  onClose,
  onOpen,
}: {
  onClose: () => void;
  onOpen: (distro: string, path: string) => void | Promise<void>;
}) {
  const translate = useStore((state) => state.t);
  const [distros, setDistros] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [distro, setDistro] = useState<string | null>(null);
  const [path, setPath] = useState("");
  const [validation, setValidation] = useState<Validation>({ kind: "idle" });
  const [error, setError] = useState<string | null>(null);

  const loadDistros = useCallback(async () => {
    setLoading(true);
    try {
      const names = await ipc.listWslDistros();
      setDistros(names);
      setDistro((current) => current ?? names[0] ?? null);
    } catch (failure) {
      setError(ipc.describeError(failure).detail);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadDistros();
  }, [loadDistros]);

  // A pasted UNC path fills both fields and is validated immediately.
  useEffect(() => {
    const split = splitUncPath(path);
    if (split) {
      setDistro(split.distro);
      setPath(split.path);
    }
  }, [path]);

  // Validate as the user types, but only once both halves are present.
  useEffect(() => {
    if (!distro || !path) {
      setValidation({ kind: "idle" });
      return;
    }
    if (!isAbsoluteLinuxPath(path)) {
      setValidation({ kind: "failed", detail: t(translate, "wsl-path-not-absolute") });
      return;
    }
    let cancelled = false;
    setValidation({ kind: "checking" });
    const timer = window.setTimeout(() => {
      void ipc
        .probeWslRepository(distro, path)
        .then(() => {
          if (!cancelled) {
            setValidation({ kind: "ok" });
          }
        })
        .catch((failure) => {
          if (!cancelled) {
            setValidation({
              kind: "failed",
              detail: ipc.describeError(failure).detail,
            });
          }
        });
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [distro, path, translate]);

  const canOpen =
    distro !== null && path.length > 0 && validation.kind === "ok";

  const status = (() => {
    if (validation.kind === "checking") {
      return (
        <span className="row muted" data-testid="wsl-checking">
          <Spinner size={11} /> {t(translate, "wsl-checking")}
        </span>
      );
    }
    if (validation.kind === "ok") {
      return (
        <span className="status-add" data-testid="wsl-check-ok">
          {t(translate, "wsl-check-ok")}
        </span>
      );
    }
    if (validation.kind === "failed") {
      return (
        <span className="status-conflict" data-testid="wsl-check-failed">
          {validation.detail}
        </span>
      );
    }
    return <span className="muted">{t(translate, "wsl-path-hint")}</span>;
  })();

  return (
    <DialogCard
      testId="wsl-dialog"
      title={t(translate, "wsl-open-title")}
      icon={<Icon name="file" size={16} />}
      onBackdrop={onClose}
      width={460}
      body={
        <>
          <div className="row">
            <span className="settings__label" style={{ width: 90 }}>
              {t(translate, "wsl-distro-label")}
            </span>
            {loading ? (
              <Spinner size={13} />
            ) : distros.length === 0 ? (
              <span className="muted" data-testid="wsl-no-distros">
                {t(translate, "wsl-no-distros")}
              </span>
            ) : (
              <select
                className="select__trigger"
                style={{ flex: 1 }}
                value={distro ?? ""}
                data-testid="wsl-distro"
                onChange={(event) => setDistro(event.target.value)}
              >
                {distros.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </select>
            )}
            <button
              type="button"
              className="tool-button tool-button--compact"
              data-testid="wsl-refresh"
              onClick={() => void loadDistros()}
            >
              {t(translate, "wsl-refresh-distros")}
            </button>
          </div>
          <div className="row">
            <span className="settings__label" style={{ width: 90 }}>
              {t(translate, "wsl-path-label")}
            </span>
            <TextInput
              value={path}
              onChange={setPath}
              autoFocus
              monospace
              placeholder="/home/dev/repo"
              testId="wsl-path"
              onSubmit={() => {
                if (canOpen && distro) {
                  void onOpen(distro, path);
                }
              }}
              onEscape={onClose}
            />
          </div>
          {status}
          {error ? <div className="status-conflict">{error}</div> : null}
          <Checkbox
            checked={true}
            disabled
            onChange={() => undefined}
            label={ta(translate, "wsl-path-hint", { path: "" })}
          />
        </>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={onClose}
            data-testid="wsl-cancel"
          >
            {t(translate, "dialog-cancel")}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            disabled={!canOpen}
            data-testid="wsl-confirm"
            onClick={() => {
              if (canOpen && distro) {
                void onOpen(distro, path);
              }
            }}
          >
            {t(translate, "wsl-open-confirm")}
          </button>
        </>
      }
    />
  );
}
