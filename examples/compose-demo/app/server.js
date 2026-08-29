// compose-demo 的镜像内应用（docker.builds: demo-node 构建用）。
const http = require("http");

const name = process.env.SVC_NAME || "demo-node";
const port = Number(process.env.PORT || 3000);

const server = http.createServer((req, res) => {
  res.writeHead(200, { "content-type": "application/json" });
  res.end(JSON.stringify({ service: name, url: req.url, ok: true }));
});

server.listen(port, "0.0.0.0", () => {
  console.log(`[${name}] listening on 0.0.0.0:${port}`);
});
