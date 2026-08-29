// 零依赖 Node 测试服务：只用内置 http 模块，无需 npm install。
// SuperTask 注入 PORT env（服务 port 字段跟随）；监听 127.0.0.1（本机工作台纪律）。
const http = require("http");

const name = process.env.SVC_NAME || "web";
const port = Number(process.env.PORT || 3000);
let hits = 0;

const server = http.createServer((req, res) => {
  hits += 1;
  const url = new URL(req.url, `http://${req.headers.host}`);
  console.log(`[${name}] ${req.method} ${url.pathname} #${hits}`);

  if (url.pathname === "/health") {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ ok: true, name, hits, port }));
    return;
  }
  if (url.pathname.startsWith("/api/echo")) {
    // 经网关反代时核对 Host / X-Forwarded-* 透传
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ service: name, method: req.method, url: req.url, headers: req.headers }, null, 2));
    return;
  }
  if (url.pathname === "/api/slow") {
    // 模拟慢启动：/api/slow?ms=8000，用于测试停止/健康超时
    const ms = Math.min(Number(url.searchParams.get("ms") || 5000), 60000);
    setTimeout(() => {
      res.writeHead(200, { "content-type": "text/plain" });
      res.end(`slow done after ${ms}ms`);
    }, ms);
    return;
  }
  if (url.pathname === "/api/fail") {
    // 模拟 500：日志页应出现 stderr/异常文本
    res.writeHead(500, { "content-type": "text/plain" });
    res.end("boom: intentional 500 for log testing");
    return;
  }
  res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  res.end(
    `<html><body style="font-family:system-ui;max-width:36rem;margin:3rem auto">` +
      `<h1>${name}</h1><p>port ${port}, hit #${hits}</p><ul>` +
      `<li><a href="/health">/health</a></li>` +
      `<li><a href="/api/echo">/api/echo</a>（回显请求头，网关透传核对）</li>` +
      `<li><a href="/api/slow?ms=3000">/api/slow?ms=3000</a></li>` +
      `<li><a href="/api/fail">/api/fail</a>（500）</li>` +
      `</ul></body></html>`,
  );
});

server.listen(port, "127.0.0.1", () => {
  console.log(`[${name}] listening on http://127.0.0.1:${port}`);
});
