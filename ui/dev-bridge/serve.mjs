import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { attachBridge } from "./bridge.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DIST = path.resolve(process.env.FERRY_UI_DIST ?? path.join(HERE, "..", "dist"));
const PORT = Number(process.env.PORT ?? 5170);

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
};

function send(res, status, body, headers = {}) {
  res.writeHead(status, headers);
  res.end(body);
}

function serveIndex(res) {
  fs.readFile(path.join(DIST, "index.html"), (err, data) => {
    if (err) {
      send(res, 500, "ferry: built UI not found — did the image build run `pnpm --filter @ferry/ui build`?");
      return;
    }
    send(res, 200, data, { "content-type": MIME[".html"] });
  });
}

const server = http.createServer((req, res) => {
  let pathname;
  try {
    pathname = decodeURIComponent(new URL(req.url ?? "/", "http://localhost").pathname);
  } catch {
    send(res, 400, "bad request");
    return;
  }
  if (pathname === "/") pathname = "/index.html";

  const filePath = path.join(DIST, pathname);
  if (filePath !== DIST && !filePath.startsWith(DIST + path.sep)) {
    send(res, 403, "forbidden");
    return;
  }

  fs.readFile(filePath, (err, data) => {
    if (err) {
      serveIndex(res);
      return;
    }
    const ext = path.extname(filePath);
    send(res, 200, data, { "content-type": MIME[ext] ?? "application/octet-stream" });
  });
});

attachBridge(server);

server.listen(PORT, () => {
  console.log(`ferry: serving the UI + daemon bridge on http://0.0.0.0:${PORT}`);
});
