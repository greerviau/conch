import { useEffect, useState } from "react";
import { Close, Folder, Plus } from "./icons";
import { RepositoryTerms } from "./Terms";
import { folderName } from "./types";
import type { IndexRow, Repository } from "./types";
export function ago(time: number, now: number) {
  const seconds = Math.max(0, now - time);
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} hr ago`;
  return `${Math.floor(seconds / 86400)} d ago`;
}
function Exclusions({
  name,
  value,
  onChange,
}: {
  name: string;
  value: string[];
  onChange: (value: string[]) => void;
}) {
  const [text, setText] = useState(value.join("\n"));
  const parse = (lines: string) =>
    lines
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean);
  // A discarded draft changes the patterns from outside; typed text that
  // already parses to the current patterns is left alone so lines can be typed.
  useEffect(() => {
    setText((current) =>
      parse(current).join("\n") === value.join("\n")
        ? current
        : value.join("\n"),
    );
  }, [value]);
  return (
    <div className="exclusions">
      <label>
        <span>Extra exclusions - one pattern per line</span>
        <textarea
          aria-label={`Extra exclusions for ${name}`}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            onChange(parse(e.target.value));
          }}
        />
      </label>
    </div>
  );
}
export function Repositories({
  repositories,
  indexes,
  now = Date.now() / 1000,
  refreshing,
  onChange,
  onChoose,
  onRefresh,
}: {
  repositories: Repository[];
  indexes: IndexRow[];
  now?: number;
  refreshing: boolean;
  onChange: (repositories: Repository[]) => void;
  onChoose: () => void;
  onRefresh: () => void;
}) {
  const [terms, setTerms] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [manual, setManual] = useState(false);
  const [path, setPath] = useState("");
  const browsing = repositories.find((r) => r.id === terms);
  if (browsing)
    return (
      <RepositoryTerms
        id={browsing.id}
        name={folderName(browsing.root)}
        refreshed={indexes.find((x) => x[0] === browsing.id)?.[2]}
        onBack={() => setTerms(null)}
      />
    );
  const add = () => {
    if (!path.trim()) return;
    onChange([
      ...repositories,
      {
        id: crypto.randomUUID(),
        root: path.trim(),
        selected: true,
        exclusions: [],
      },
    ]);
    setPath("");
    setManual(false);
  };
  const manualEntry = manual ? (
    <div className="manual">
      <input
        aria-label="Repository path"
        value={path}
        onChange={(e) => setPath(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && add()}
        placeholder="/Users/me/work/project"
      />
      <button onClick={add}>Add path</button>
    </div>
  ) : null;
  const pathLink = (
    <button className="link" onClick={() => setManual(!manual)}>
      Enter a path instead
    </button>
  );
  if (repositories.length === 0)
    return (
      <>
        <div className="card empty">
          <span className="tile">
            <Folder size={22} />
          </span>
          <h2>No repositories yet</h2>
          <p>
            Dictation and global mappings still work. Add a repository to get
            your project&apos;s names spelled right.
          </p>
          <button className="primary with-icon" onClick={onChoose}>
            <Plus />
            Add repository
          </button>
          {pathLink}
        </div>
        {manualEntry}
      </>
    );
  return (
    <>
      <p className="intro">
        Conch reads filenames and code declarations from these folders so it can
        spell your terms correctly. Ignored, binary, generated, oversized, and
        external symlink files are excluded.
      </p>
      <div className="card list">
        {repositories.map((r) => {
          const status = indexes.find((x) => x[0] === r.id);
          const name = folderName(r.root);
          const tag = !status
            ? ["", "Not indexed yet"]
            : !status[1].available
              ? ["bad", "Folder not found"]
              : status[1].incomplete
                ? ["warn", "Partial index"]
                : ["ok", "Ready"];
          return (
            <div key={r.id}>
              <div className="repo">
                <span className="tile">
                  <Folder />
                </span>
                <div className="meta">
                  <b>{name}</b>
                  <span className="path">{r.root}</span>
                  <div className="info">
                    <span className={`tag ${tag[0]}`}>{tag[1]}</span>
                    {status?.[1].available && (
                      <span>
                        {`${status[1].entries.toLocaleString()} terms - updated ${ago(status[2], now)}${
                          status[1].skipped
                            ? ` - ${status[1].skipped.toLocaleString()} symbol extractions skipped`
                            : ""
                        }`}
                      </span>
                    )}
                  </div>
                </div>
                <button
                  className="ghost"
                  aria-label={`Terms for ${name}`}
                  disabled={!status?.[1].available}
                  onClick={() => setTerms(r.id)}
                >
                  Terms
                </button>
                <button
                  className="ghost"
                  aria-label={`Exclusions for ${name}`}
                  aria-expanded={open === r.id}
                  onClick={() => setOpen(open === r.id ? null : r.id)}
                >
                  Exclusions
                </button>
                <button
                  className="icon"
                  aria-label={`Remove ${name}`}
                  onClick={() =>
                    onChange(repositories.filter((v) => v.id !== r.id))
                  }
                >
                  <Close />
                </button>
              </div>
              {open === r.id && (
                <Exclusions
                  name={name}
                  value={r.exclusions}
                  onChange={(exclusions) =>
                    onChange(
                      repositories.map((v) =>
                        v.id === r.id ? { ...v, exclusions } : v,
                      ),
                    )
                  }
                />
              )}
            </div>
          );
        })}
        <div className="add-row">
          <button className="primary with-icon" onClick={onChoose}>
            <Plus />
            Add repository
          </button>
          <button className="ghost" disabled={refreshing} onClick={onRefresh}>
            Refresh indexes
          </button>
          <span className="spacer" />
          {pathLink}
        </div>
        {manualEntry}
      </div>
    </>
  );
}
