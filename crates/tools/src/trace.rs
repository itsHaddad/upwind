//! Traces air from the command line, reading wind files from disk.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Instant;

use upwind_core::codec::decode;
use upwind_core::tracer::ROW_WIDTH;
use upwind_core::{Start, Status, Tracer, WindStore};

use crate::gfs::Result;
use crate::time::format_utc;
use crate::timeline::{self, Published};

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
    pub files_read: usize,
    pub bytes_read: u64,
    pub seconds: f64,
}

/// Runs a trace, loading wind files from disk as the tracer asks for them.
pub fn run(data: &Path, starts: &[Start], time: Option<i64>, hours: u32) -> Result<Traced> {
    let days = timeline::published(data)?;
    let times: Vec<i64> = days.iter().flat_map(|d| d.times.iter().copied()).collect();
    let last = *times.last().ok_or("the data directory holds no wind")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    let start_time = time.unwrap_or_else(|| now.min(last).div_euclid(3600) * 3600);

    let day_of =
        |valid: i64| -> Option<&Published> { days.iter().find(|d| d.times.contains(&valid)) };
    let wind = timeline::wind_dir(data);
    let mut store = WindStore::new(times.clone());
    let mut tracer = Tracer::new(starts, start_time as f64, hours);
    let started = Instant::now();
    let mut read = HashSet::new();
    let mut bytes_read = 0;
    let mut asked_before = Vec::new();
    loop {
        match tracer.advance(&store, u32::MAX) {
            Status::Done => break,
            Status::Running => {}
            Status::Need(keys) => {
                if keys.iter().all(|key| store.has(*key)) || keys == asked_before {
                    return Err("the wind files do not hold what the trace needs".into());
                }
                asked_before.clone_from(&keys);
                for (valid, tile, band) in keys {
                    let day = day_of(valid).ok_or("tracer asked for an unknown time")?;
                    let path = timeline::file_path(&wind.join(&day.dir), tile, band);
                    if !read.insert(path.clone()) {
                        continue;
                    }
                    let bytes = fs::read(&path)?;
                    bytes_read += bytes.len() as u64;
                    for chunk in decode(&bytes)? {
                        store.add(chunk);
                    }
                }
            }
        }
    }
    Ok(Traced {
        start_time,
        parcels: starts.len(),
        points: tracer.points().to_vec(),
        files_read: read.len(),
        bytes_read,
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

/// The values of one parcel in one row.
fn row(values: &[f32], parcel: usize) -> [f32; ROW_WIDTH] {
    let at = parcel * ROW_WIDTH;
    values[at..at + ROW_WIDTH].try_into().unwrap()
}

fn print_table(traced: &Traced, options: &Options) {
    let stride = traced.parcels * ROW_WIDTH;
    for (p, height) in options.heights_m.iter().enumerate() {
        println!(
            "air arriving {height:.0} m above {:.3}N {:.3}E",
            options.lat, options.lon
        );
        println!("  hours  time (UTC)         lat      lon     hPa   above ground");
        for (hour, values) in traced.points.chunks(stride).enumerate() {
            if !(hour as u32).is_multiple_of(options.every) {
                continue;
            }
            let [lon, lat, hpa, height] = row(values, p);
            if lon.is_nan() {
                println!("  {:>5}  beyond the stored history", -(hour as i64));
                break;
            }
            let lon = if lon > 180.0 { lon - 360.0 } else { lon };
            println!(
                "  {:>5}  {}  {:>7.2}  {:>7.2}  {:>6.0}  {:>6.0} m",
                -(hour as i64),
                format_utc(traced.start_time - hour as i64 * 3600),
                lat,
                lon,
                hpa,
                height
            );
        }
    }
    eprintln!(
        "{} files ({:.2} MB) read, {:.0} ms",
        traced.files_read,
        traced.bytes_read as f64 / 1e6,
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
                .map(|values| row(values, p))
                .filter(|[lon, ..]| !lon.is_nan())
                .map(|[lon, lat, hpa, height]| format!("[{lon:.4},{lat:.4},{hpa:.1},{height:.0}]"))
                .collect();
            format!("{{\"height_m\":{height},\"path\":[{}]}}", path.join(","))
        })
        .collect();
    format!(
        "{{\"start_time\":{},\"step_s\":3600,\"columns\":[\"lon\",\"lat\",\"hpa\",\"height_m\"],\
         \"files\":{},\"bytes\":{},\"parcels\":[{}]}}",
        traced.start_time,
        traced.files_read,
        traced.bytes_read,
        parcels.join(",")
    )
}
