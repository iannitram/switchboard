import { useEffect, useState } from "react";
import { call } from "../core";
import type { CoreError } from "../core";

// CORE-5 (#8): the three paths the core reads from the settings file, as `settings_get` and
// `settings_save` send them (commands.rs `SettingsView`).
export type SettingName = "pipeline_repo" | "nextcloud_backups" | "vault_agents";
export type SettingsValues = Record<SettingName, string | null>;
export type SettingsView = { settings: SettingsValues; missing: readonly string[] };

// In the core's SETTING_NAMES order.
const FIELDS: readonly { name: SettingName; label: string }[] = [
  { name: "pipeline_repo", label: "Pipeline repo clone" },
  { name: "nextcloud_backups", label: "NextCloud backups folder" },
  { name: "vault_agents", label: "Vault agents folder" },
];

const labelOf = (name: string) => FIELDS.find((f) => f.name === name)?.label ?? name;

function fieldsFrom(view: SettingsView): Record<SettingName, string> {
  return {
    pipeline_repo: view.settings.pipeline_repo ?? "",
    nextcloud_backups: view.settings.nextcloud_backups ?? "",
    vault_agents: view.settings.vault_agents ?? "",
  };
}

function valuesFrom(fields: Record<SettingName, string>): SettingsValues {
  const value = (text: string) => (text.trim() === "" ? null : text.trim());
  return {
    pipeline_repo: value(fields.pipeline_repo),
    nextcloud_backups: value(fields.nextcloud_backups),
    vault_agents: value(fields.vault_agents),
  };
}

// The Settings view (CORE-5): each path, which are not set, and a Save. A core error shows in
// place (SHELL-3).
export function Settings() {
  const [view, setView] = useState<SettingsView | null>(null);
  const [fields, setFields] = useState<Record<SettingName, string>>({
    pipeline_repo: "",
    nextcloud_backups: "",
    vault_agents: "",
  });
  const [error, setError] = useState<CoreError | null>(null);
  const [saved, setSaved] = useState(false);
  const [loaded, setLoaded] = useState(false);

  const show = (answer: SettingsView) => {
    setView(answer);
    setFields(fieldsFrom(answer));
  };

  useEffect(() => {
    let live = true;
    call<SettingsView>("settings_get").then((res) => {
      if (!live) return;
      setLoaded(true);
      if ("error" in res) setError(res.error);
      else {
        show(res.ok);
        setError(null);
      }
    });
    return () => {
      live = false;
    };
  }, []);

  const save = async () => {
    setSaved(false);
    const res = await call<SettingsView>("settings_save", { settings: valuesFrom(fields) });
    if ("error" in res) {
      setError(res.error);
    } else {
      show(res.ok);
      setError(null);
      setSaved(true);
    }
  };

  const edit = (name: SettingName, text: string) => {
    setFields((f) => ({ ...f, [name]: text }));
    setSaved(false);
  };

  return (
    <section aria-label="Settings">
      {error && (
        <div role="alert">
          <strong>{error.command} failed</strong>
          {error.lines.map((line, i) => (
            <p key={i}>{line}</p>
          ))}
        </div>
      )}
      {!loaded ? (
        <p>loading settings</p>
      ) : (
        <>
          {FIELDS.map(({ name, label }) => (
            <p key={name}>
              <label>
                {label}{" "}
                <input type="text" value={fields[name]} onChange={(e) => edit(name, e.target.value)} />
              </label>
            </p>
          ))}
          {view && (
            <p>
              {view.missing.length === 0
                ? "All paths set"
                : `Not set: ${view.missing.map(labelOf).join(", ")}`}
            </p>
          )}
          <button onClick={save}>Save</button>
          {saved && <p>Saved</p>}
        </>
      )}
    </section>
  );
}
