const fs = require("fs"), path = require("path");
const skip = ["src/ipc/mock.ts", "src/ipc/protocol.ts", "src/i18n/locales"];
const files = [];
(function walk(d) {
  for (const e of fs.readdirSync(d, { withFileTypes: true })) {
    const p = path.join(d, e.name);
    if (e.isDirectory()) walk(p);
    else if (/\.(tsx?|css)$/.test(e.name)) files.push(p);
  }
})("src");
const re = /[\u4e00-\u9fff]/;
for (const f of files) {
  const rel = path.relative(".", f).split(path.sep).join("/");
  if (skip.some((s) => rel.split(path.sep).join("/").startsWith(s))) continue;
  const lines = fs.readFileSync(f, "utf8").split("\n");
  lines.forEach((l, i) => {
    if (!re.test(l)) return;
    // 去掉行注释、JSX 注释、块注释延续行；再要求中文出现在字符串字面量内
    let code = l;
    code = code.replace(/\{\/\*.*\*\/\}/g, "");
    if (re.test(code)) code = code.replace(/\/\/.*$/, "");
    if (re.test(code)) code = code.replace(/\/\*.*$/, "");
    if (re.test(code) && /^\s*\*/.test(l)) return;
    if (!re.test(code)) return;
    // 只关心字符串字面量里的中文（模板串/单双引号）
    const lit = code.match(/(["'`])[^"'`]*[\u4e00-\u9fff][^]*?\1|`[^`]*[\u4e00-\u9fff][^`]*`/);
    if (lit) console.log(rel + ":" + (i + 1) + ": " + l.trim().slice(0, 160));
  });
}
