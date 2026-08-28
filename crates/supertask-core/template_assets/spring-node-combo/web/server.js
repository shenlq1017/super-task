// SuperTask 官方模板：零依赖 Node HTTP 服务（仅用 node:http，无需 npm install）
const http = require('node:http');

const PORT = process.env.PORT || 5173;

const server = http.createServer((req, res) => {
  const path = (req.url || '/').split('?')[0];
  if (path === '/' || path === '/healthz') {
    res.writeHead(200, { 'Content-Type': 'application/json; charset=utf-8' });
    res.end(JSON.stringify({ ok: true, path }));
    return;
  }
  res.writeHead(404, { 'Content-Type': 'application/json; charset=utf-8' });
  res.end(JSON.stringify({ ok: false, path }));
});

server.listen(PORT, () => {
  console.log(`[demo-web] listening on http://127.0.0.1:${PORT}`);
});
