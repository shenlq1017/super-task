/**
 * 一次性/可重复脚本：从 `src/i18n/locales/zh-CN.ts`（源语言）经 opencc-js
 * 「简 → 繁（台湾用语，s2twp）」生成 `src/i18n/locales/zh-TW.ts` 静态资源。
 * 运行时不依赖 opencc-js。再生成：`node scripts/gen-zh-hant.mjs`。
 * 生成底稿后可对常见术语做人工校对（如「伺服器/網路」由 twp 词组覆盖大部分）。
 */
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import * as OpenCC from "opencc-js";

const here = dirname(fileURLToPath(import.meta.url));
const srcPath = join(here, "..", "src", "i18n", "locales", "zh-CN.ts");
const outPath = join(here, "..", "src", "i18n", "locales", "zh-TW.ts");

const raw = readFileSync(srcPath, "utf8");
const start = raw.indexOf("const zhCN =");
const end = raw.lastIndexOf("};");
if (start < 0 || end < 0) throw new Error("cannot locate zhCN object literal");
const literal = raw.slice(start + "const zhCN =".length, end + 1);
const zhCN = new Function(`return ${literal}`)();

/** 深转换：只转换叶子字符串。 */
function convert(node, convertor) {
  if (typeof node === "string") return convertor(node);
  if (Array.isArray(node)) return node.map((v) => convert(v, convertor));
  if (node && typeof node === "object") {
    const out = {};
    for (const [k, v] of Object.entries(node)) out[k] = convert(v, convertor);
    return out;
  }
  return node;
}

function serialize(node, indent = 2) {
  const pad = " ".repeat(indent);
  if (typeof node === "string") return JSON.stringify(node);
  if (Array.isArray(node)) return `[${node.map((v) => serialize(v, indent)).join(", ")}]`;
  const entries = Object.entries(node);
  if (entries.length === 0) return "{}";
  return "{\n" + entries.map(([k, v]) => `${pad}"${k}": ${serialize(v, indent + 2)},`).join("\n") + "\n" + " ".repeat(indent - 2) + "}";
}

// s2twp：简繁转换 + 台湾惯用词（伺服器、網路、設定…）
const convertor = OpenCC.Converter({ from: "cn", to: "twp" });
const zhTW = convert(zhCN, convertor);

// 人工校对层：opencc 词组未覆盖的常用术语（重新生成时保留）。
const TERM_FIXES = [
  [/映象/g, "映像"], // docker image：台湾惯用「映像」
];
(function fix(node) {
  for (const [k, v] of Object.entries(node)) {
    if (typeof v === "string") {
      node[k] = TERM_FIXES.reduce((acc, [re, to]) => acc.replace(re, to), v);
    } else if (v && typeof v === "object") {
      fix(v);
    }
  }
})(zhTW);

const banner = `/**
 * zh-TW 資源：由 scripts/gen-zh-hant.mjs（opencc-js s2twp）從 zh-CN 生成底稿，
 * 允許人工校對術語；重新生成會覆蓋本檔案（1.4 規格 §6.2）。
 */
const zhTW = ${serialize(zhTW)};

export default zhTW;
`;
writeFileSync(outPath, banner, "utf8");

// 顺手做四语 key 一致性校验（en-US / ja-JP 用同一 eval 技巧加载）
function loadLocale(file) {
  const text = readFileSync(join(here, "..", "src", "i18n", "locales", file), "utf8");
  const m = text.match(/const \w+ =/);
  const last = text.lastIndexOf("};");
  if (!m || last < 0) throw new Error(`cannot parse ${file}`);
  return new Function(`return ${text.slice(m.index + m[0].length, last + 1)}`)();
}

function flatten(node, prefix = "") {
  const out = [];
  for (const [k, v] of Object.entries(node ?? {})) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === "object" && !Array.isArray(v)) out.push(...flatten(v, key));
    else out.push(key);
  }
  return out;
}

const base = new Set(flatten(zhCN));
let missingTotal = 0;
for (const file of ["en-US.ts", "ja-JP.ts", "zh-TW.ts"]) {
  const keys = new Set(flatten(loadLocale(file)));
  const missing = [...base].filter((k) => !keys.has(k));
  const extra = [...keys].filter((k) => !base.has(k));
  if (missing.length || extra.length) {
    missingTotal += missing.length + extra.length;
    console.error(`[locales] ${file}: missing=${missing.length} extra=${extra.length}`);
    for (const k of missing.slice(0, 20)) console.error("  - " + k);
    for (const k of extra.slice(0, 20)) console.error("  + " + k);
  } else {
    console.log(`[locales] ${file}: ${keys.size} keys, parity OK`);
  }
}
console.log(`zh-TW generated (${base.size} keys). parity issues: ${missingTotal}`);
if (missingTotal > 0) process.exitCode = 1;
