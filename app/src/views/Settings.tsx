import { useEffect, useState } from "react";
import { call, type CoreError, type CoreResult } from "../core";

// CORE-5: the Settings view. Shows the three paths, says which are not set, and saves them
// through settings_get and settings_save (commands.rs). Plain text fields: no folder picker.
type SettingName = "pipeline_repo" | "nextcloud_backups" | "vault_agents";
type SettingsView = { settings: Record<SettingName, string | null>; missing: string[] };
type Fields = Record<SettingName, string>;

const SETTINGS: { name: SettingName; label: string }[] = [
  { name: "pipeline_repo", label: "Pipeline repo clone" },
  { name: "nextcloud_backups", label: "NextCloud backups folder" },
  { name: "vault_agents", label: "Vault agents folder" },
];
const BLANK: Fields = { pipeline_repo: "", nextcloud_backups: "", vault_agents: "" };

const labelOf = (name: string) => SETTINGS.find((s) => s.name === name)?.label ?? name;

function fieldsOf(view: SettingsView): Fields {
  return {
    pipeline_repo: view.settings.pipeline_repo ?? "",
    nextcloud_backups: view.settings.nextcloud_backups ?? "",
    vault_agents: view.settings.vault_agents ?? "",
  };
}

function Failed({ error }: { error: CoreError }) {
  return (
    <div role="alert">
      <p>{error.command} failed</p>
      {error.lines.map((line, i) => (
        <p key={i}>{line}</p>
      ))}
    </div>
  );
}

export function Settings() {
  const [answer, setAnswer] = useState<CoreResult<SettingsView> | null>(null);
  const [fields, setFields] = useState<Fields>(BLANK);
  const [saved, setSaved] = useState(false);
  const [saveError, setSaveError] = useState<CoreError | null>(null);

  useEffect(() => {
    let live = true;
    call<SettingsView>("settings_get").then((r) => {
      if (!live) return;
      setAnswer(r);
      if ("ok" in r) setFields(fieldsOf(r.ok));
    });
    return () => {
      live = false;
    };
  }, []);

  function edit(name: SettingName, value: string) {
    setFields((f) => ({ ...f, [name]: value }));
    setSaved(false);
  }

  function save() {
    const settings = Object.fromEntries(
      SETTINGS.map(({ name }) => [name, fields[name].trim() || null]),
    ) as Record<SettingName, string | null>;
    setSaveError(null);
    setSaved(false);
    call<SettingsView>("settings_save", { settings }).then((r) => {
      if ("error" in r) {
        setSaveError(r.error);
        return;
      }
      setAnswer(r);
      setFields(fieldsOf(r.ok));
      setSaved(true);
    });
  }

  if (!answer) {
    return (
      <section aria-label="Settings">
        <p>loading settings</p>
      </section>
    );
  }

  if ("error" in answer) {
    return (
      <section aria-label="Settings">
        <Failed error={answer.error} />
      </section>
    );
  }

  const view = answer.ok;
  return (
    <section aria-label="Settings">
      {SETTINGS.map(({ name, label }) => (
        <label key={name}>
          <span>{label}</span>
          <input type="text" value={fields[name]} onChange={(e) => edit(name, e.target.value)} />
        </label>
      ))}
      <p>
        {view.missing.length === 0
          ? "All paths set"
          : `Not set: ${view.missing.map(labelOf).join(", ")}`}
      </p>
      <button type="button" onClick={save}>
        Save
      </button>
      {saved && <p>Saved</p>}
      {saveError && <Failed error={saveError} />}
    </section>
  );
}
