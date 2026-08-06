import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";

interface ExclusionsProps {
  paths: string[] | null;
  onAdd: (path: string) => void;
  onRemove: (path: string) => void;
}

function Exclusions({ paths, onAdd, onRemove }: ExclusionsProps) {
  const [draft, setDraft] = useState("");

  if (paths === null) return <p>Reading your exclusion list…</p>;

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const path = draft.trim();
    if (path) {
      onAdd(path);
      setDraft("");
    }
  };

  return (
    <>
      {paths.length === 0 ? (
        <p>Nothing is excluded. Spiral Clean will consider everything its categories cover.</p>
      ) : (
        <ul>
          {paths.map((path) => (
            <li key={path}>
              <code>{path}</code>
              <button type="button" onClick={() => onRemove(path)}>
                Stop excluding {path}
              </button>
            </li>
          ))}
        </ul>
      )}
      <form onSubmit={submit}>
        <label htmlFor="exclusion-path">Full path to never touch</label>
        <input
          id="exclusion-path"
          type="text"
          value={draft}
          placeholder="/Users/you/Library/Caches/something"
          onChange={(event) => setDraft(event.target.value)}
        />
        <button type="submit" disabled={draft.trim() === ""}>
          Add exclusion
        </button>
      </form>
    </>
  );
}

export default function Settings() {
  const [fda, setFda] = useState<boolean | null>(null);
  const [paths, setPaths] = useState<string[] | null>(null);
  const [version, setVersion] = useState<string>("");
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    invoke<boolean>("fda_status").then(setFda).catch(() => setFda(null));
    invoke<string[]>("exclusions_list")
      .then(setPaths)
      .catch((e) => {
        // An unreadable list is not an empty one — and while it is
        // unreadable, `remove.rs` denies every removal. Saying so here is
        // the difference between "nothing happened" and "here is why".
        setPaths([]);
        setError(`${e}`);
      });
    getVersion().then(setVersion).catch(() => setVersion(""));
  }, []);

  useEffect(load, [load]);

  const change = (command: string, path: string) => {
    setError(null);
    invoke<string[]>(command, { path })
      .then(setPaths)
      .catch((e) => setError(`${e}`));
  };

  return (
    <section>
      <h1>Settings</h1>
      {error && <p role="alert">{error}</p>}

      <h2>Full Disk Access</h2>
      {fda === null ? (
        <p>Spiral Clean could not check whether it has Full Disk Access.</p>
      ) : fda ? (
        <p>Granted. Spiral Clean can see everything it needs to.</p>
      ) : (
        <>
          <p>Not granted. Without it Spiral Clean cannot see most of what it cleans.</p>
          <button
            type="button"
            onClick={() => invoke("open_privacy_settings").catch((e) => setError(`${e}`))}
          >
            Open Privacy &amp; Security
          </button>
        </>
      )}

      <h2>Never touch these</h2>
      <Exclusions
        paths={paths}
        onAdd={(path) => change("exclusions_add", path)}
        onRemove={(path) => change("exclusions_remove", path)}
      />

      <h2>Updates</h2>
      <p>
        Spiral Clean does not check for updates yet, and makes no network connections of any kind.
      </p>

      <h2>About</h2>
      <p>Spiral Clean {version}</p>
    </section>
  );
}
