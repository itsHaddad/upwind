# Upwind

**Where has your air been?** Type any place on Earth and watch the air above it travel backwards through the last ten days, traced through real wind data by a Rust engine running in your browser.

Upwind takes a point 500 m above the place you choose and follows it back, hour by hour, through NOAA's global wind fields. The result is drawn on a globe as the path that air took to reach you, with a short account of it: where it was each day, how far it came, how high it rose.

## How it works

```text
NOAA GFS model output            upwind ingest (Rust)             browser
(GRIB2 on NOAA's open data  ──▶  byte-range download of     ──▶  WebAssembly engine fetches
 bucket, a new run every 6 h)    wind, omega, surface             only the chunks the air
                                 pressure and temperature;        passes through and
                                 packed into small 3D chunks      integrates the path
```

- **The engine** (`crates/core`) is about a thousand lines of Rust with one small dependency. It holds the wind file format, interpolation, and the trajectory integrator: the predictor-corrector scheme NOAA's [HYSPLIT](https://www.ready.noaa.gov/HYSPLIT.php) uses, with winds averaged as Earth-centred vectors so paths stay accurate on curved flows and across the poles. It compiles unchanged to native code and to an 87 kB WebAssembly module.
- **The wind files.** The 1° global grid on 17 pressure levels is cut three ways: into 15° tiles, into three altitude bands, and into days. One file holds one tile and band for one day (about 35 kB). A trace asks for a chunk only when the air enters it, so a ten-day trace of eight parcels downloads a few megabytes out of a history of three hundred. Files are named after the model output they were built from, so a file never changes once published; a day that has been repacked keeps its old files for one more refresh, for pages that are already open.
- **The pipeline** (`crates/tools`) reads NOAA's index files and downloads only the fields it needs by byte range (about 4 MB per valid time), caches them, and repacks only the days that changed. A full ten days builds in a few minutes; an update after a new model run takes seconds.
- **The page** (`web/`) is TypeScript with no framework: a WebGL2 globe, the engine wrapper, and the story text. Place names along the path come from a 29 kB lookup grid built from Natural Earth.

## Run it

You need [Rust](https://rustup.rs) with the `wasm32-unknown-unknown` target and [Bun](https://bun.sh).

```sh
bun install
cargo build --release -p upwind-tools
./target/release/upwind ingest          # ten days of wind into ./data (≈ 600 MB, a few minutes)
bun scripts/build-regions.ts            # the place-name grid
bun run build                           # engine to WebAssembly, page to web/dist
bun run dev                             # http://localhost:4173 (HOST=0.0.0.0 to reach it from another machine)
```

From the command line:

```sh
./target/release/upwind trace --lat 55.70 --lon 13.19 --height 500 --hours 240
```

`bun run check` runs the formatters, linters, type check and every test.

## Hosting it

The page is static files (`web/dist`). The wind is static files too (`data/wind`, about 260 MB), refreshed four times a day by `.github/workflows/wind.yml`. They can live on different hosts: build the page with `UPWIND_WIND_BASE=https://your-wind-host` and serve the wind with `Access-Control-Allow-Origin: *`. A trace downloads a few megabytes, so choose a wind host that does not cut you off after a fixed amount of traffic.

## How far to trust it

A back trajectory is an estimate that degrades with every hour it goes back. Upwind draws several fainter paths around the main one (other heights, slightly different starting points) to show how much the answer depends on exactly where the air starts.

We compared six 120-hour trajectories (Lund, Denver and Singapore at 500 m and 1500 m) with HYSPLIT on NOAA READY:

| Compared with | Median horizontal difference, as a share of distance travelled |
| --- | --- |
| HYSPLIT on 1° GDAS wind (same resolution as ours) | 23% |
| HYSPLIT on 0.25° GFS wind | 43% |
| For scale: HYSPLIT on 1° wind against HYSPLIT on 0.25° wind | 28% |

So against the same-resolution reference Upwind sits inside the usual 15–30% spread between trajectory models, and the larger gap to the 0.25° run is mostly the resolution of the wind, which separates HYSPLIT from itself by a similar amount.

Known limits:

- Heights near steep terrain are less reliable. Over the Rockies our parcels rose about half as fast as HYSPLIT's in the first hours. HYSPLIT moves air in a terrain-following coordinate; Upwind works in pressure.
- The start height and the heights shown use the surface temperature and a standard lapse rate, not the full temperature profile.
- There is no turbulent mixing. This is the path of an air mass, not of each molecule in it.
- 1° wind every three hours misses sea breezes, valley winds and thunderstorms.

## Layout

```text
crates/core    grid, wind file codec, interpolation, integrator (no I/O)
crates/tools   the `upwind` command: ingest, trace, synth
crates/wasm    the engine behind a C interface, for the browser
web/           the page
scripts/       build, dev server, place-name grid
```

## Credits

- Wind: NOAA Global Forecast System, from the [NOAA Open Data Dissemination](https://registry.opendata.aws/noaa-gfs-bdp-pds/) programme.
- Method and reference: NOAA Air Resources Laboratory's HYSPLIT model (Stein et al., 2015, *Bull. Amer. Meteor. Soc.*).
- Coastlines, borders and sea names: [Natural Earth](https://www.naturalearthdata.com) and [world-atlas](https://github.com/topojson/world-atlas).
- Place search: [Open-Meteo](https://open-meteo.com) geocoding, built on GeoNames.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your option.
