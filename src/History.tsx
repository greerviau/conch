import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Search } from "./icons";
import { Banner } from "./ui";
import type { HistoryPage } from "./types";
export const PAGE_SIZE = 50;
export function History({
  enabled,
  revision,
  onSettings,
}: {
  /** Whether Keep history is on, so new results are saved. */
  enabled: boolean;
  /** Changes when a result is saved, so the list reloads. */
  revision: number;
  onSettings: () => void;
}) {
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [offset, setOffset] = useState(0);
  // The page on screen with the offset it was requested at. A failed request
  // leaves the previous page and its own range in place.
  const [page, setPage] = useState<HistoryPage & { offset: number }>();
  const [reload, setReload] = useState(0);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [confirming, setConfirming] = useState(false);
  useEffect(() => {
    // Only a changed search text restarts from the first page.
    const next = input.trim();
    if (next === query) return;
    const timer = window.setTimeout(() => {
      setQuery(next);
      setOffset(0);
    }, 150);
    return () => window.clearTimeout(timer);
  }, [input, query]);
  useEffect(() => {
    let stale = false;
    invoke<HistoryPage>("history_page", { query, offset, limit: PAGE_SIZE })
      .then((next) => {
        if (stale) return;
        // Deleting the last entry of a page leaves it empty; go back one page.
        if (next.items.length === 0 && next.total > 0 && offset > 0) {
          setOffset(Math.max(0, offset - PAGE_SIZE));
          return;
        }
        setPage({ ...next, offset });
        setError("");
      })
      .catch((e) => {
        if (!stale) setError(String(e));
      });
    return () => {
      stale = true;
    };
  }, [query, offset, reload, revision]);
  async function run(name: string, args?: Record<string, unknown>) {
    setError("");
    setNotice("");
    try {
      await invoke(name, args);
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    }
  }
  async function copy(id: number, raw: boolean) {
    if (await run("history_copy", { id, raw }))
      setNotice(raw ? "Copied the raw transcript" : "Copied");
  }
  async function remove(id: number) {
    if (await run("history_delete", { id })) setReload((n) => n + 1);
  }
  async function clear() {
    setConfirming(false);
    if (await run("history_clear")) {
      setOffset(0);
      setReload((n) => n + 1);
    }
  }
  const filtered = query !== "";
  // Asking for the offset that already failed has to fetch again.
  const go = (next: number) => {
    if (next === offset) setReload((n) => n + 1);
    else setOffset(next);
  };
  const total = page?.total ?? 0;
  const shown = page?.offset ?? 0;
  const from = total ? shown + 1 : 0;
  const to = shown + (page?.items.length ?? 0);
  return (
    <div className="history">
      <p className="intro">
        Results Conch delivered, newest first. Copying an entry puts its text on
        the clipboard and never pastes it.
      </p>
      {!enabled && (
        <p className="notice" role="status">
          History is off. New results are not saved. Turn on Keep history in
          Settings.{" "}
          <button className="link" onClick={onSettings}>
            Open Settings
          </button>
        </p>
      )}
      {error && (
        <Banner kind="error" onDismiss={() => setError("")}>
          {error}
        </Banner>
      )}
      {notice && (
        <Banner kind="notice" onDismiss={() => setNotice("")}>
          {notice}
        </Banner>
      )}
      {page && total === 0 && !filtered ? (
        <p className="empty-note">
          {enabled ? "No results saved yet" : "Nothing saved"}
        </p>
      ) : (
        <>
          <div className="terms-tools">
            <label className="search">
              <Search />
              <input
                aria-label="Search history"
                placeholder="Search history"
                value={input}
                onChange={(e) => setInput(e.target.value)}
              />
            </label>
            {confirming ? (
              <span className="actions">
                <span>
                  {filtered
                    ? "Delete all entries, including those that do not match this search?"
                    : `Delete all ${total} entries?`}
                </span>
                <button onClick={() => setConfirming(false)}>Cancel</button>
                <button className="danger" onClick={() => void clear()}>
                  Delete all
                </button>
              </span>
            ) : (
              <button className="ghost" onClick={() => setConfirming(true)}>
                Clear history
              </button>
            )}
          </div>
          {page && total === 0 ? (
            <p className="empty-note">No matches</p>
          ) : (
            <div className="card list">
              {page?.items.map((entry) => (
                <div className="entry" key={entry.id}>
                  <div className="entry-head">
                    <time
                      dateTime={new Date(entry.created * 1000).toISOString()}
                    >
                      {new Date(entry.created * 1000).toLocaleString()}
                    </time>
                    <span className="actions">
                      <button onClick={() => void copy(entry.id, false)}>
                        Copy
                      </button>
                      {entry.raw !== entry.text && (
                        <button onClick={() => void copy(entry.id, true)}>
                          Copy raw
                        </button>
                      )}
                      <button
                        className="ghost"
                        onClick={() => void remove(entry.id)}
                      >
                        Delete
                      </button>
                    </span>
                  </div>
                  <p className="entry-text">{entry.text}</p>
                </div>
              ))}
            </div>
          )}
          {page && total > 0 && (
            <div className="pager">
              <span>{`${from}-${to} of ${total.toLocaleString()} ${filtered ? "matches" : "entries"}`}</span>
              <span className="actions">
                <button
                  disabled={shown === 0}
                  onClick={() => go(Math.max(0, shown - PAGE_SIZE))}
                >
                  Previous
                </button>
                <button
                  disabled={shown + PAGE_SIZE >= total}
                  onClick={() => go(shown + PAGE_SIZE)}
                >
                  Next
                </button>
              </span>
            </div>
          )}
        </>
      )}
    </div>
  );
}
