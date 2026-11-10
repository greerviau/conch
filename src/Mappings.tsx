import { Arrow, Close, Plus } from "./icons";
import { Toggle } from "./ui";
import { folderName } from "./types";
import type { Mapping, Repository } from "./types";
export function Mappings({
  mappings,
  repositories,
  onChange,
}: {
  mappings: Mapping[];
  repositories: Repository[];
  onChange: (mappings: Mapping[]) => void;
}) {
  const names = repositories.map((r) => folderName(r.root));
  const label = (r: Repository, i: number) =>
    names.filter((n) => n === names[i]).length > 1 ? r.root : names[i];
  const update = (index: number, patch: Partial<Mapping>) =>
    onChange(mappings.map((m, i) => (i === index ? { ...m, ...patch } : m)));
  const add = () =>
    onChange([
      ...mappings,
      { phrase: "", replacement: "", repository: null, enabled: true },
    ]);
  return (
    <>
      <p className="intro">
        Replace a spoken phrase with exact text. The longest phrase wins, and
        repository mappings override global ones. Conflicts wait for your
        choice.
      </p>
      {mappings.length === 0 ? (
        <div className="card empty">
          <h2>No mappings yet</h2>
          <p>
            Add one when Conch keeps mishearing a name, for example &quot;git
            hub&quot; to GitHub.
          </p>
          <button className="primary with-icon" onClick={add}>
            <Plus />
            Add mapping
          </button>
        </div>
      ) : (
        <div className="card">
          <div className="mapping head" aria-hidden="true">
            <span />
            <span>You say</span>
            <span />
            <span>Conch writes</span>
            <span>Scope</span>
            <span />
          </div>
          {mappings.map((m, i) => (
            <div className={`mapping${m.enabled ? "" : " off"}`} key={i}>
              <Toggle
                label="Enabled"
                checked={m.enabled}
                onChange={(enabled) => update(i, { enabled })}
              />
              <input
                aria-label="Spoken phrase"
                value={m.phrase}
                placeholder="Spoken phrase"
                onChange={(e) => update(i, { phrase: e.target.value })}
              />
              <span className="arrow" aria-hidden="true">
                <Arrow />
              </span>
              <input
                aria-label="Replacement"
                className="code"
                value={m.replacement}
                placeholder="Replacement"
                onChange={(e) => update(i, { replacement: e.target.value })}
              />
              <select
                aria-label="Scope"
                value={m.repository ?? ""}
                onChange={(e) =>
                  update(i, { repository: e.target.value || null })
                }
              >
                <option value="">Global</option>
                {repositories.map((r, n) => (
                  <option key={r.id} value={r.id}>
                    {label(r, n)}
                  </option>
                ))}
              </select>
              <button
                className="icon"
                aria-label="Remove mapping"
                onClick={() => onChange(mappings.filter((_, n) => n !== i))}
              >
                <Close />
              </button>
            </div>
          ))}
          <div className="add-row">
            <button className="with-icon" onClick={add}>
              <Plus />
              Add mapping
            </button>
          </div>
        </div>
      )}
    </>
  );
}
