// SuperTask demo web frontend: plain http server (no dependencies).
const http = require("http");

const PORT = process.env.PORT || 5173;

const server = http.createServer((req, res) => {
  res.setHeader("Content-Type", "application/json");
  res.end(JSON.stringify({ service: "web", backend: "http://127.0.0.1:8081" }));
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`web listening on http://127.0.0.1:${PORT}`);
});
