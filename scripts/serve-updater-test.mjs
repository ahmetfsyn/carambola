import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import path from "node:path";

const root = path.resolve(process.argv[2] ?? "src-tauri/target-updater-test-host");
const rootPrefix = root.endsWith(path.sep) ? root : `${root}${path.sep}`;

const server = createServer((request, response) => {
  if (request.method !== "GET" && request.method !== "HEAD") {
    response.writeHead(405, { Allow: "GET, HEAD" }).end();
    return;
  }

  let pathname;
  try {
    pathname = decodeURIComponent(new URL(request.url ?? "/", "http://127.0.0.1").pathname);
  } catch {
    response.writeHead(400).end("Bad request");
    return;
  }

  const file = path.resolve(root, `.${pathname}`);
  if (file !== root && !file.startsWith(rootPrefix)) {
    response.writeHead(403).end("Forbidden");
    return;
  }

  let stats;
  try {
    stats = statSync(file);
    if (!stats.isFile()) throw new Error("not a file");
  } catch {
    response.writeHead(404).end("Not found");
    return;
  }

  const contentType = file.endsWith(".json")
    ? "application/json; charset=utf-8"
    : file.endsWith(".zip")
      ? "application/zip"
      : "application/octet-stream";
  response.writeHead(200, {
    "Content-Length": stats.size,
    "Content-Type": contentType,
    "Cache-Control": "no-store",
  });

  if (request.method === "HEAD") {
    response.end();
    return;
  }

  createReadStream(file).pipe(response);
});

server.listen(8765, "127.0.0.1", () => {
  console.log(`Updater test host serving ${root} at http://127.0.0.1:8765`);
  console.log("Loopback only; Ctrl+C stops the test host.");
});

server.on("error", (error) => {
  console.error(`Could not start updater test host: ${error.message}`);
  process.exitCode = 1;
});
