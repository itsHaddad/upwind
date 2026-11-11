//! Writes a small made-up wind history, for tests that must not depend on
//! the network or on today's weather.

use std::fs;
use std::path::Path;

use upwind_core::build::GlobalFields;

use crate::gfs::{Result, LEVELS_HPA};
use crate::timeline::{self, Entry};

/// The cycle the made-up history starts at: 2026-01-01 00 UTC.
pub const CYCLE: i64 = 1_767_225_600;
/// The history covers this many hours, one valid time every three.
pub const HOURS: u32 = 48;

/// A wavy westerly flow that changes slowly with time.
fn wind(lon: f64, lat: f64, pressure: f32, hour: f64) -> (f32, f32, f32) {
    let (l, f) = (lon.to_radians(), lat.to_radians());
    let aloft = 1.0 + f64::from(100_000.0 - pressure) / 50_000.0;
    let pulse = 1.0 + 0.2 * (hour / 12.0).sin();
    let u = (18.0 * f.cos() + 6.0 * (2.0 * l).sin() * f.cos()) * aloft * pulse;
    let v = 7.0 * (3.0 * l + hour / 20.0).sin() * f.cos() * aloft;
    let w = 0.04 * (2.0 * l).cos() * f.cos();
    (u as f32, v as f32, w as f32)
}

pub fn run(data: &Path) -> Result<()> {
    let levels_pa: Vec<f32> = LEVELS_HPA.iter().map(|h| *h as f32 * 100.0).collect();
    let mut entries = Vec::new();
    for forecast_hour in (0..=HOURS).step_by(3) {
        let entry = Entry {
            cycle: CYCLE,
            forecast_hour,
        };
        let hour = f64::from(forecast_hour);
        let fields = GlobalFields::from_fn(
            entry.valid_time(),
            &levels_pa,
            |lon, lat, p| wind(lon, lat, p, hour),
            |lon, _| 100_500.0 + 800.0 * (lon.to_radians()).sin() as f32,
        );
        fs::create_dir_all(data.join(entry.dir()))?;
        for tile in fields.tiles() {
            fs::write(timeline::tile_path(data, &entry, tile.tile), tile.encode(6))?;
        }
        entries.push(entry);
    }
    timeline::write_manifest(data, &entries, CYCLE)?;
    Ok(())
}
