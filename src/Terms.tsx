import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Back, Search } from "./icons";
import type { TermPage } from "./types";
export const PAGE_SIZE = 100;
type Kind = "all" | "files" | "symbols";
const KINDS: [Kind, string][] = [
  ["all", "All"],
  ["files", "Files"],
  ["symbols", "Symbols"],
];
function Highlight({ text, query }: { text: string; query: string }) {
  if (!query) return <>{text}</>;
  const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return (
    <>
      {text
        .split(new RegExp(`(${escaped})`, "i"))
        .map((part, i) => (i % 2 ? <mark key={i}>{part}</mark> : part))}
    </>
  );
}
export function RepositoryTerms({
  id,
  name,
  refreshed,
  onBack,
}: {
  id: string;
  name: string;
  /** Changes when the repository is indexed again, so the list reloads. */
  refreshed?: number;
  onBack: () => void;
}) {
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<Kind>("all");
  const [offset, setOffset] = useState(0);
  // The page on screen with the offset it was requested at. A failed request
  // leaves the previous page and its own range in place.
  const [page, setPage] = useState<TermPage & { offset: number }>();
  const [reload, setReload] = useState(0);
  const [error, setError] = useState("");
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
    invoke<TermPage>("index_terms", {
      repository: id,
      query,
      kind,
      offset,
      limit: PAGE_SIZE,
    })
      .then((next) => {
        if (stale) return;
        setPage({ ...next, offset });
        setError("");
      })
      .catch((e) => {
        if (!stale) setError(String(e));
      });
    return () => {
      stale = true;
    };
  }, [id, query, kind, offset, reload, refreshed]);
  const filtered = query !== "" || kind !== "all";
  const total = page?.total ?? 0;
  const shown = page?.offset ?? 0;
  const from = total ? shown + 1 : 0;
  const to = shown + (page?.items.length ?? 0);
  // Asking for the offset that already failed has to fetch again.
  const go = (next: number) => {
    if (next === offset) setReload((n) => n + 1);
    else setOffset(next);
  };
  return (
    <div className="terms">
      <button className="crumb" onClick={onBack}>
        <Back />
        <span>Repositories</span>
        <span aria-hidden="true">/</span>
        <b>{name}</b>
      </button>
      <p className="intro">
        These are the names Conch uses to correct your dictation: file paths,
        file names, and function, class and type names.
      </p>
      <div className="terms-tools">
        <label className="search">
          <Search />
          <input
            aria-label="Search terms"
            placeholder="Search terms"
            value={input}
            onChange={(e) => setInput(e.target.value)}
          />
        </label>
        <div className="segmented">
          {KINDS.map(([value, label]) => (
            <button
              key={value}
              aria-pressed={kind === value}
              className={kind === value ? "on" : ""}
              onClick={() => {
                setKind(value);
                setOffset(0);
              }}
            >
              {label}
            </button>
          ))}
        </div>
      </div>
      {error && (
        <div role="alert" className="error">
          {error}
        </div>
      )}
      {page && total === 0 ? (
        <p className="empty-note">
          {filtered ? "No matches" : "No terms indexed yet"}
        </p>
      ) : (
        <>
          <div className="card list">
            {page?.items.map((term, i) => (
              <div className="term" key={`${shown}-${i}`}>
                <code>
                  <Highlight text={term.text} query={query} />
                </code>
                <span className="path">{term.file}</span>
              </div>
            ))}
          </div>
          {page && (
            <div className="pager">
              <span>{`${from}-${to} of ${total.toLocaleString()} ${filtered ? "matches" : "terms"}`}</span>
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
