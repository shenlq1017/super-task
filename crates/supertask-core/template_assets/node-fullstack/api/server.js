// SuperTask 官方模板：零依赖 Node API 服务（仅用 node:http，无需 npm install）
const http = require('node:http');

const PORT = process.env.PORT || 3001;

const server = http.createServer((req, res) => {
  const path = (req.url || '/').split('?')[0];
  if (path === '/api/health' || path === '/healthz') {
    res.writeHead(200, { 'Content-Type': 'application/json; charset=utf-8' });
    res.end(JSON.stringify({ ok: true, service: 'api', path }));
    return;
  }
  res.writeHead(200, { 'Content-Type': 'application/json; charset=utf-8' });
  res.end(JSON.stringify({ ok: true, service: 'api', path, message: 'hello from SuperTask api' }));
});

server.listen(PORT, () => {
  console.log(`[demo-api] listening on http://127.0.0.1:${PORT}`);
});
