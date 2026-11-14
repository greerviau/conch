import { Fragment, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Search } from "./icons";
import { Banner } from "./ui";
import type { LogEntry, LogPage } from "./types";
export const PAGE_SIZE = 50;
const LEVELS = [
  ["all", "All"],
  ["warn", "Warnings"],
  ["error", "Errors"],
] as const;
const LABELS: Record<LogEntry["level"], [string, string]> = {
  info: ["info", "Info"],
  warn: ["warn", "Warning"],
  error: ["bad", "Error"],
};
const clock = (ms: number) =>
  new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
/** `Today`, `Yesterday`, or the date, in the local time zone. */
function dayLabel(ms: number, now = new Date()) {
  const day = new Date(ms);
  const start = (d: Date) =>
    new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((start(now) - start(day)) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return day.toLocaleDateString([], {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}
function size(bytes: number) {
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1000))} KB`;
}
/** The entries as plain text for a bug report, with times in UTC. */
function asText(items: LogEntry[]) {
  return items
    .map((e) => {
      const head = `${new Date(e.time).toISOString()} ${e.level.toUpperCase()} ${e.source} ${e.message}`;
      const detail = e.detail
        ? e.detail
            .split("\n")
            .map((line) => `  ${line}`)
            .join("\n")
        : "";
      return detail ? `${head}\n${detail}` : head;
    })
    .join("\n")
    .concat("\n");
}
/** `text` with each match of `query` in a mark. */
function marked(text: string, query: string) {
  if (!query) return text;
  const lower = text.toLowerCase();
  const needle = query.toLowerCase();
  const parts = [];
  let from = 0;
  for (
    let at = lower.indexOf(needle);
    at >= 0;
    at = lower.indexOf(needle, from)
  ) {
    parts.push(
      text.slice(from, at),
      <mark key={at}>{text.slice(at, at + needle.length)}</mark>,
    );
    from = at + needle.length;
  }
  parts.push(text.slice(from));
  return parts;
}
export function Logs({
  revision,
}: {
  /** Changes when the log changes, so the list reloads. */
  revision: number;
}) {
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [level, setLevel] = useState<(typeof LEVELS)[number][0]>("all");
  const [source, setSource] = useState("all");
  const [offset, setOffset] = useState(0);
  // The page on screen with the offset it was requested at. A failed request
  // leaves the previous page and its own range in place.
  const [page, setPage] = useState<LogPage & { offset: number }>();
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
    invoke<LogPage>("logs_page", {
      level,
      source,
      query,
      offset,
      limit: PAGE_SIZE,
    })
      .then((next) => {
        if (stale) return;
        // Clearing can leave the offset past the end; go back a page.
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
  }, [level, source, query, offset, reload, revision]);
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
  const narrow = (apply: () => void) => {
    apply();
    setOffset(0);
  };
  async function clear() {
    setConfirming(false);
    if (await run("logs_clear")) {
      setOffset(0);
      setReload((n) => n + 1);
    }
  }
  const all = page?.all ?? 0;
  const total = page?.total ?? 0;
  const shown = page?.offset ?? 0;
  const from = total ? shown + 1 : 0;
  const to = shown + (page?.items.length ?? 0);
  const filtered = query !== "" || level !== "all" || source !== "all";
  // Asking for the offset that already failed has to fetch again.
  const go = (next: number) => {
    if (next === offset) setReload((n) => n + 1);
    else setOffset(next);
  };
  let day = "";
  return (
    <div className="logs">
      <p className="intro">
        What Conch did and why, newest first. Logs never contain audio,
        transcripts, result text, or source text, and paths are shortened.
      </p>
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
      {page && all === 0 ? (
        <p className="empty-note">
          Nothing logged yet. Entries appear here as you dictate.
        </p>
      ) : (
        <>
          <div className="terms-tools">
            <label className="search">
              <Search />
              <input
                aria-label="Search logs"
                placeholder="Search logs"
                value={input}
                onChange={(e) => setInput(e.target.value)}
              />
            </label>
            <div className="segmented" role="group" aria-label="Level">
              {LEVELS.map(([value, label]) => (
                <button
                  key={value}
                  className={level === value ? "on" : ""}
                  aria-pressed={level === value}
                  onClick={() => narrow(() => setLevel(value))}
                >
                  {label}
                </button>
              ))}
            </div>
            <select
              aria-label="Source"
              value={source}
              onChange={(e) => narrow(() => setSource(e.target.value))}
            >
              <option value="all">All sources</option>
              {page?.sources.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
          </div>
          {page && total === 0 ? (
            <p className="empty-note">No matches</p>
          ) : (
            <div className="card list">
              {page?.items.map((entry, i) => {
                const [tone, label] = LABELS[entry.level];
                const heading = dayLabel(entry.time);
                const line = (
                  <span className="log-line">
                    <time dateTime={new Date(entry.time).toISOString()}>
                      {clock(entry.time)}
                    </time>
                    <span className={`tag ${tone}`}>{label}</span>
                    <span className="src">{entry.source}</span>
                    <span className="msg">{marked(entry.message, query)}</span>
                  </span>
                );
                const group = heading !== day;
                day = heading;
                return (
                  <Fragment key={`${shown}-${i}`}>
                    {group && <h3 className="day">{heading}</h3>}
                    {entry.detail ? (
                      <details
                        className={`log${entry.level === "error" ? " err" : ""}`}
                      >
                        <summary>{line}</summary>
                        <div className="detail">
                          <pre>{marked(entry.detail, query)}</pre>
                        </div>
                      </details>
                    ) : (
                      <div
                        className={`log${entry.level === "error" ? " err" : ""}`}
                      >
                        {line}
                      </div>
                    )}
                  </Fragment>
                );
              })}
            </div>
          )}
          {page && total > 0 && (
            <div className="pager">
              <span>
                {`${from}-${to} of ${total.toLocaleString()} ${filtered ? "matches" : "entries"}`}
                {` - conch.log, ${size(page.bytes)}`}
              </span>
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
          <div className="foot">
            {confirming ? (
              <span className="confirm">
                <span>
                  Delete all log entries, including those hidden by the filters?
                </span>
                <button onClick={() => setConfirming(false)}>Cancel</button>
                <button className="danger" onClick={() => void clear()}>
                  Delete all
                </button>
              </span>
            ) : (
              <span className="actions">
                <button
                  disabled={!page?.items.length}
                  onClick={async () => {
                    const items = page?.items ?? [];
                    if (await run("logs_copy", { text: asText(items) }))
                      setNotice(
                        `Copied ${items.length} ${items.length === 1 ? "entry" : "entries"}`,
                      );
                  }}
                >
                  Copy shown entries
                </button>
                <button onClick={() => void run("logs_reveal")}>
                  Show log file
                </button>
                <button className="ghost" onClick={() => setConfirming(true)}>
                  Clear logs
                </button>
              </span>
            )}
          </div>
        </>
      )}
    </div>
  );
}
