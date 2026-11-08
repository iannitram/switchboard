import { useEffect, useState } from "react";
import { call, type CoreError, type CoreResult } from "../core";

// CORE-5: the settings as the core answers `settings_get` and `settings_save` (commands.rs
// `SettingsView`). `missing` names the paths not set, in SETTING_NAMES order.
type SettingName = "pipeline_repo" | "nextcloud_backups" | "vault_agents";
type SettingsValues = Record<SettingName, string | null>;
type SettingsView = { settings: SettingsValues; missing: string[] };

const SETTING_NAMES: readonly SettingName[] = ["pipeline_repo", "nextcloud_backups", "vault_agents"];

const LABELS: Record<SettingName, string> = {
  pipeline_repo: "Pipeline repo clone",
  nextcloud_backups: "NextCloud backups folder",
  vault_agents: "Vault agents folder",
};

const label = (name: string) => LABELS[name as SettingName] ?? name;

function fieldsOf(settings: SettingsValues): Record<SettingName, string> {
  return {
    pipeline_repo: settings.pipeline_repo ?? "",
    nextcloud_backups: settings.nextcloud_backups ?? "",
    vault_agents: settings.vault_agents ?? "",
  };
}

// CORE-5: the Settings view. Shows each path, says which are not set, and saves what I type
// through the core. A core error shows in place.
export function Settings() {
  const [view, setView] = useState<SettingsView | null>(null);
  const [fields, setFields] = useState<Record<SettingName, string>>(fieldsOf({
    pipeline_repo: null,
    nextcloud_backups: null,
    vault_agents: null,
  }));
  const [error, setError] = useState<CoreError | null>(null);
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);

  const answer = (result: CoreResult<SettingsView>): boolean => {
    if ("error" in result) {
      setError(result.error);
      return false;
    }
    setError(null);
    setView(result.ok);
    setFields(fieldsOf(result.ok.settings));
    return true;
  };

  useEffect(() => {
    let live = true;
    call<SettingsView>("settings_get").then((result) => {
      if (live) answer(result);
    });
    return () => {
      live = false;
    };
  }, []);

  const edit = (name: SettingName, value: string) => {
    setFields((f) => ({ ...f, [name]: value }));
    setSaved(false);
  };

  const save = async () => {
    const settings = Object.fromEntries(
      SETTING_NAMES.map((name) => {
        const text = fields[name].trim();
        return [name, text === "" ? null : text];
      }),
    ) as SettingsValues;
    setSaved(false);
    setSaving(true);
    const result = await call<SettingsView>("settings_save", { settings });
    setSaving(false);
    setSaved(answer(result));
  };

  return (
    <section aria-label="Settings">
      {error && (
        <div role="alert">
          <p>{error.command} failed</p>
          {error.lines.map((line, i) => (
            <p key={i}>{line}</p>
          ))}
        </div>
      )}
      {view === null && error === null && <p>loading settings</p>}
      {view !== null && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          {SETTING_NAMES.map((name) => (
            <p key={name}>
              <label>
                {LABELS[name]}
                <input type="text" value={fields[name]} onChange={(e) => edit(name, e.target.value)} />
              </label>
            </p>
          ))}
          <p>
            {view.missing.length === 0 ? "All paths set" : `Not set: ${view.missing.map(label).join(", ")}`}
          </p>
          <button type="submit" disabled={saving}>
            Save
          </button>
          {saved && <p>Saved</p>}
        </form>
      )}
    </section>
  );
}
