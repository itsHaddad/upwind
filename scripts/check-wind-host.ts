// Checks that a wind host serves what the page relies on. Run after
// publishing: bun scripts/check-wind-host.ts https://your-wind-host
//
// The page depends on three things the host's configuration decides: other
// sites may read the files, the manifest is not remembered for long (the
// page also asks for a fresh copy every time), and a
// file that is gone is answered with 404 (that is how a page that has been
// open across a refresh learns its manifest is out of date).

export {};

const site = process.argv[2]?.replace(/\/$/, "");
if (!site) {
  console.error("usage: bun scripts/check-wind-host.ts https://your-wind-host");
  process.exit(2);
}

const problems: string[] = [];
const expect = (ok: boolean, problem: string) => {
  if (!ok) problems.push(problem);
};

/** The longest time, in seconds, a response says it may be reused. */
function maxAge(response: Response): number {
  const ages = [...(response.headers.get("cache-control") ?? "").matchAll(/max-age=(\d+)/g)].map((m) => Number(m[1]));
  return ages.length ? Math.max(...ages) : 0;
}

function checkReadable(response: Response, what: string): void {
  const origin = response.headers.get("access-control-allow-origin");
  expect(origin === "*", `${what}: Access-Control-Allow-Origin is ${JSON.stringify(origin)}, expected exactly "*"`);
}

const manifestResponse = await fetch(`${site}/manifest.json`, { cache: "no-store" });
expect(manifestResponse.status === 200, `manifest: status ${manifestResponse.status}`);
checkReadable(manifestResponse, "manifest");
expect(
  maxAge(manifestResponse) <= 3600,
  `manifest: may be reused for ${maxAge(manifestResponse)} s; it must stay fresh`,
);

const manifest = (await manifestResponse.json().catch(() => undefined)) as
  | { days?: { dir: string; times: number[] }[] }
  | undefined;
const newest = manifest?.days?.at(-1);
expect(newest !== undefined, "manifest: lists no days");

if (newest) {
  const file = await fetch(`${site}/${newest.dir}/0/0.uwb`, { cache: "no-store" });
  expect(file.status === 200, `wind file: status ${file.status}`);
  checkReadable(file, "wind file");
  const magic = new TextDecoder().decode((await file.arrayBuffer()).slice(0, 3));
  expect(magic === "UWB", `wind file: does not start like a wind file (${JSON.stringify(magic)})`);

  const gone = await fetch(`${site}/19000101.00000000/0/0.uwb`, { cache: "no-store" });
  expect(gone.status === 404, `missing file: status ${gone.status}, expected 404`);
  checkReadable(gone, "missing file");

  const age = Date.now() / 1000 - Math.max(...newest.times);
  expect(age < 18 * 3600, `the newest wind is ${(age / 3600).toFixed(0)} hours old`);
}

if (problems.length > 0) {
  console.error(`${site} is not serving the wind correctly:\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
console.log(`${site} serves the wind correctly.`);
