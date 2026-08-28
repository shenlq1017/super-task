export type EnvImportFormat = "auto" | "env" | "yaml" | "properties" | "json";

function stripQuotes(v: string): string {
  const t = v.trim();
  if ((t.startsWith('"') && t.endsWith('"')) || (t.startsWith("'") && t.endsWith("'"))) {
    return t.slice(1, -1);
  }
  return t;
}

function parseEnvLines(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const body = line.startsWith("export ") ? line.slice(7).trim() : line;
    const eq = body.indexOf("=");
    if (eq <= 0) continue;
    const key = body.slice(0, eq).trim();
    if (!key) continue;
    out[key] = stripQuotes(body.slice(eq + 1));
  }
  return out;
}

function parsePropertiesLines(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#") || line.startsWith("!")) continue;
    const sep = line.includes("=") ? "=" : line.includes(":") ? ":" : null;
    if (!sep) continue;
    const idx = line.indexOf(sep);
    const key = line.slice(0, idx).trim();
    if (!key) continue;
    out[key] = stripQuotes(line.slice(idx + 1));
  }
  return out;
}

function parseYamlEnv(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const raw of text.split(/\r?\n/)) {
    const m = raw.match(/^(\s*)([A-Za-z_][\w.-]*)\s*:\s*(.*)$/);
    if (!m) continue;
    const indent = m[1].length;
    if (indent > 2) continue;
    const key = m[2];
    let val = m[3].trim();
    if (!val || val === "|" || val === ">") continue;
    if (val.startsWith("#")) continue;
    out[key] = stripQuotes(val);
  }
  return out;
}

function parseJsonEnv(text: string): Record<string, string> {
  const parsed = JSON.parse(text) as unknown;
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
    if (v == null) out[k] = "";
    else if (typeof v === "string" || typeof v === "number" || typeof v === "boolean") out[k] = String(v);
  }
  return out;
}

function detectFormat(text: string): EnvImportFormat {
  const t = text.trim();
  if (!t) return "env";
  if (t.startsWith("{") || t.startsWith("[")) return "json";
  if (/^[A-Za-z_][\w.-]*\s*:/m.test(t) && !/=/.test(t.split("\n")[0] ?? "")) return "yaml";
  if (/=/.test(t)) return "env";
  return "properties";
}

export function parseEnvImport(text: string, format: EnvImportFormat = "auto"): Record<string, string> {
  const trimmed = text.trim();
  if (!trimmed) return {};
  const fmt = format === "auto" ? detectFormat(trimmed) : format;
  try {
    switch (fmt) {
      case "json":
        return parseJsonEnv(trimmed);
      case "yaml":
        return parseYamlEnv(trimmed);
      case "properties":
        return parsePropertiesLines(trimmed);
      case "env":
      default:
        return parseEnvLines(trimmed);
    }
  } catch {
    return {};
  }
}
