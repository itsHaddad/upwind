// Builds the page into web/dist. Run with: bun scripts/build-web.ts

import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const web = join(root, "web");
const dist = join(web, "dist");

rmSync(dist, { recursive: true, force: true });
mkdirSync(dist, { recursive: true });

const built = await Bun.build({
  entrypoints: [join(web, "src/main.ts")],
  outdir: dist,
  target: "browser",
  define: { "process.env.UPWIND_WIND_BASE": JSON.stringify(process.env.UPWIND_WIND_BASE ?? "./data") },
  minify: true,
  sourcemap: "linked",
});
if (!built.success) {
  for (const log of built.logs) console.error(log);
  process.exit(1);
}

for (const file of ["index.html", "about.html", "styles.css", "about.css"]) cpSync(join(web, file), join(dist, file));
cpSync(join(web, "public"), dist, { recursive: true });
cpSync(join(root, "node_modules/world-atlas/countries-50m.json"), join(dist, "countries-50m.json"));
for (const needed of ["engine.wasm", "regions.bin", "regions.json"]) {
  if (!existsSync(join(dist, needed))) {
    console.error(`web/public/${needed} is missing: run "bun run wasm" and "bun scripts/build-regions.ts" first.`);
    process.exit(1);
  }
}
const size = built.outputs.reduce((sum, output) => sum + (output.path.endsWith(".js") ? output.size : 0), 0);
console.log(`built web/dist (${(size / 1000).toFixed(0)} kB of script)`);
