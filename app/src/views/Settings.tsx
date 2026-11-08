import { useEffect, useState, type FormEvent } from "react";
import { call, type CoreError, type CoreResult } from "../core";

// CORE-5: the Settings view. It shows the three paths, says which are not set, and saves them
// through `settings_get` and `settings_save`. The core decides what counts as set; this view
// only trims the text and sends `null` for an empty field.

// The settings names, in the core's order (`SETTING_NAMES`, crates/core/src/settings.rs).
const SETTING_NAMES = ["pipeline_repo", "nextcloud_backups", "vault_agents"] as const;
type SettingName = (typeof SETTING_NAMES)[number];

const LABELS: Record<SettingName, string> = {
  pipeline_repo: "Pipeline repo clone",
  nextcloud_backups: "NextCloud backups folder",
  vault_agents: "Vault agents folder",
};

type Paths = Record<SettingName, string | null>;
type Fields = Record<SettingName, string>;
type SettingsAnswer = { settings: Paths; missing: SettingName[] };

const EMPTY_FIELDS: Fields = { pipeline_repo: "", nextcloud_backups: "", vault_agents: "" };

function toFields(paths: Paths): Fields {
  return {
    pipeline_repo: paths.pipeline_repo ?? "",
    nextcloud_backups: paths.nextcloud_backups ?? "",
    vault_agents: paths.vault_agents ?? "",
  };
}

function toPaths(fields: Fields): Paths {
  return {
    pipeline_repo: fields.pipeline_repo.trim() || null,
    nextcloud_backups: fields.nextcloud_backups.trim() || null,
    vault_agents: fields.vault_agents.trim() || null,
  };
}

function summary(missing: SettingName[]): string {
  if (missing.length === 0) return "All paths set";
  return `Not set: ${missing.map((name) => LABELS[name]).join(", ")}`;
}

export function Settings() {
  const [answer, setAnswer] = useState<SettingsAnswer | null>(null);
  const [fields, setFields] = useState<Fields>(EMPTY_FIELDS);
  const [error, setError] = useState<CoreError | null>(null);
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let live = true;
    call<SettingsAnswer>("settings_get").then((result: CoreResult<SettingsAnswer>) => {
      if (!live) return;
      if ("error" in result) {
        setError(result.error);
        return;
      }
      setAnswer(result.ok);
      setFields(toFields(result.ok.settings));
    });
    return () => {
      live = false;
    };
  }, []);

  function edit(name: SettingName, value: string) {
    setFields((current) => ({ ...current, [name]: value }));
    setSaved(false);
  }

  function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSaving(true);
    call<SettingsAnswer>("settings_save", { settings: toPaths(fields) }).then((result) => {
      setSaving(false);
      if ("error" in result) {
        setError(result.error);
        setSaved(false);
        return;
      }
      setError(null);
      setAnswer(result.ok);
      setFields(toFields(result.ok.settings));
      setSaved(true);
    });
  }

  return (
    <section aria-label="Settings">
      {!answer && !error && <p>loading settings</p>}
      {error && (
        <div role="alert">
          <p>{`${error.command} failed`}</p>
          <ul>
            {error.lines.map((line, i) => (
              <li key={i}>{line}</li>
            ))}
          </ul>
        </div>
      )}
      {answer && (
        <form onSubmit={save}>
          {SETTING_NAMES.map((name) => (
            <label key={name}>
              {LABELS[name]}
              <input type="text" value={fields[name]} onChange={(e) => edit(name, e.target.value)} />
            </label>
          ))}
          <p>{summary(answer.missing)}</p>
          <button type="submit" disabled={saving}>
            Save
          </button>
          {saved && <p>Saved</p>}
        </form>
      )}
    </section>
  );
}
