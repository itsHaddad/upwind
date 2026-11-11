// Serves web/dist and the local wind tiles. Run with: bun scripts/dev.ts

import { join, normalize, sep } from "node:path";

const root = join(import.meta.dir, "..");
const dist = join(root, "web/dist");
const data = join(root, process.env.UPWIND_DATA ?? "data/wind");
const port = Number(process.env.PORT ?? 4173);

function inside(base: string, path: string): string | undefined {
  const full = normalize(join(base, path));
  return full === base || full.startsWith(base + sep) ? full : undefined;
}

Bun.serve({
  port,
  // Only this machine by default. HOST=0.0.0.0 opens it to the network,
  // which a browser on the Windows side of WSL needs.
  hostname: process.env.HOST ?? "127.0.0.1",
  async fetch(request) {
    const { pathname } = new URL(request.url);
    const path = decodeURIComponent(pathname);
    const target = path.startsWith("/data/")
      ? inside(data, path.slice("/data/".length))
      : inside(dist, path === "/" ? "index.html" : path.slice(1));
    const file = target ? Bun.file(target) : undefined;
    if (!file || !(await file.exists())) return new Response("Not found", { status: 404 });
    return new Response(file, { headers: { "Cache-Control": "no-store" } });
  },
});
console.log(`http://localhost:${port}`);
