//! Traces air from the command line, reading tiles from disk.

use std::fs;
use std::path::Path;
use std::time::Instant;

use upwind_core::geo::standard_altitude_m;
use upwind_core::tracer::ROW_WIDTH;
use upwind_core::{Start, Status, Tile, Tracer, WindStore};

use crate::gfs::Result;
use crate::time::format_utc;
use crate::timeline::{self, Entry};

pub struct Options<'a> {
    pub data: &'a Path,
    pub lon: f64,
    pub lat: f64,
    pub heights_m: Vec<f64>,
    pub hours: u32,
    /// Unix time to trace back from; the newest whole hour when absent.
    pub time: Option<i64>,
    /// Print every this many hours.
    pub every: u32,
    pub json: bool,
}

pub struct Traced {
    pub start_time: i64,
    pub parcels: usize,
    /// `rows × parcels × ROW_WIDTH` values.
    pub points: Vec<f32>,
    pub tiles_read: usize,
    pub seconds: f64,
}

/// Runs a trace, loading tiles from disk as the tracer asks for them.
pub fn run(data: &Path, starts: &[Start], time: Option<i64>, hours: u32) -> Result<Traced> {
    let entries = timeline::scan(data)?;
    let last = entries.last().ok_or("the data directory holds no wind")?;
    let newest_hour = |limit: i64| limit.div_euclid(3600) * 3600;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    let start_time = time.unwrap_or_else(|| newest_hour(now.min(last.valid_time())));

    let find = |valid: i64| -> Option<&Entry> { entries.iter().find(|e| e.valid_time() == valid) };
    let mut store = WindStore::new(entries.iter().map(Entry::valid_time).collect());
    let mut tracer = Tracer::new(starts, start_time as f64, hours);
    let started = Instant::now();
    let mut tiles_read = 0;
    loop {
        match tracer.advance(&store, u32::MAX) {
            Status::Done => break,
            Status::Running => {}
            Status::Need(keys) => {
                for (valid, tile) in keys {
                    let entry = find(valid).ok_or("tracer asked for an unknown time")?;
                    let bytes = fs::read(timeline::tile_path(data, entry, tile))?;
                    store.add(Tile::decode(&bytes)?);
                    tiles_read += 1;
                }
            }
        }
    }
    Ok(Traced {
        start_time,
        parcels: starts.len(),
        points: tracer.points().to_vec(),
        tiles_read,
        seconds: started.elapsed().as_secs_f64(),
    })
}

pub fn command(options: &Options) -> Result<()> {
    let starts: Vec<Start> = options
        .heights_m
        .iter()
        .map(|height_m| Start {
            lon: options.lon,
            lat: options.lat,
            height_m: *height_m,
        })
        .collect();
    let traced = run(options.data, &starts, options.time, options.hours)?;
    if options.json {
        println!("{}", to_json(&traced, &options.heights_m));
    } else {
        print_table(&traced, options);
    }
    Ok(())
}

fn print_table(traced: &Traced, options: &Options) {
    let stride = traced.parcels * ROW_WIDTH;
    for (p, height) in options.heights_m.iter().enumerate() {
        println!(
            "air arriving {height:.0} m above {:.3}N {:.3}E",
            options.lat, options.lon
        );
        println!("  hours  time (UTC)         lat      lon     hPa   altitude");
        for (row, values) in traced.points.chunks(stride).enumerate() {
            if !(row as u32).is_multiple_of(options.every) {
                continue;
            }
            let [lon, lat, hpa] = [values[p * 3], values[p * 3 + 1], values[p * 3 + 2]];
            if lon.is_nan() {
                println!("  {:>5}  beyond the stored history", -(row as i64));
                break;
            }
            let lon = if lon > 180.0 { lon - 360.0 } else { lon };
            println!(
                "  {:>5}  {}  {:>7.2}  {:>7.2}  {:>6.0}  {:>6.0} m",
                -(row as i64),
                format_utc(traced.start_time - row as i64 * 3600),
                lat,
                lon,
                hpa,
                standard_altitude_m(f64::from(hpa) * 100.0)
            );
        }
    }
    eprintln!(
        "{} tiles read, {:.0} ms",
        traced.tiles_read,
        traced.seconds * 1000.0
    );
}

fn to_json(traced: &Traced, heights_m: &[f64]) -> String {
    let stride = traced.parcels * ROW_WIDTH;
    let parcels: Vec<String> = heights_m
        .iter()
        .enumerate()
        .map(|(p, height)| {
            let path: Vec<String> = traced
                .points
                .chunks(stride)
                .filter(|values| !values[p * 3].is_nan())
                .map(|values| {
                    format!(
                        "[{:.4},{:.4},{:.1}]",
                        values[p * 3],
                        values[p * 3 + 1],
                        values[p * 3 + 2]
                    )
                })
                .collect();
            format!("{{\"height_m\":{height},\"path\":[{}]}}", path.join(","))
        })
        .collect();
    format!(
        "{{\"start_time\":{},\"step_s\":3600,\"columns\":[\"lon\",\"lat\",\"hpa\"],\"parcels\":[{}]}}",
        traced.start_time,
        parcels.join(",")
    )
}
