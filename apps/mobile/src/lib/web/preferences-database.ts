import { FRONTEND_CONTRACT, type ReaderPreferencePatch, type ReaderPreferences } from "../bindings.ts";
import { getWebDatabase } from "./database.ts";
import { applyReaderPreferencePatch } from "./wasm-core.ts";

const KEY = "reader";
const contract = FRONTEND_CONTRACT.reader_preferences;

export async function getWebReaderPreferences(): Promise<ReaderPreferences> {
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT value_json FROM web_preferences WHERE key = ?",
    bind: [KEY],
    rowMode: 0,
    returnValue: "resultRows",
  });
  return rows.length
    ? { ...contract.defaults, ...JSON.parse(String(rows[0])) } as ReaderPreferences
    : { ...contract.defaults };
}

export async function updateWebReaderPreference(patch: ReaderPreferencePatch): Promise<ReaderPreferences> {
  const preferences = await applyReaderPreferencePatch(await getWebReaderPreferences(), patch);
  const { db } = await getWebDatabase();
  db.exec({
    sql: `
      INSERT INTO web_preferences (key, value_json) VALUES (?, ?)
      ON CONFLICT (key) DO UPDATE SET value_json = excluded.value_json
    `,
    bind: [KEY, JSON.stringify(preferences)],
  });
  return preferences;
}
