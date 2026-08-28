// SuperTask 官方模板（最小起步）：零依赖 Node HTTP 服务
const http = require('node:http');

const PORT = process.env.PORT || 5173;

http.createServer((req, res) => {
  res.writeHead(200, { 'Content-Type': 'application/json; charset=utf-8' });
  res.end(JSON.stringify({ ok: true }));
}).listen(PORT, () => {
  console.log(`[demo-web] listening on http://127.0.0.1:${PORT}`);
});
