//! Writes a small made-up wind history, for tests that must not depend on
//! the network or on today's weather.

use std::fs;
use std::path::Path;
use std::sync::Mutex;

use upwind_core::build::GlobalFields;

use crate::gfs::Result;
use crate::pack;
use crate::timeline::{self, Entry};

/// The cycle the made-up history starts at: 2026-01-01 00 UTC.
pub const CYCLE: i64 = 1_767_225_600;
/// The history covers this many hours, one valid time every three.
pub const HOURS: u32 = 48;

/// A wavy westerly flow that changes slowly with time and lifts air in
/// places, so traces cross tiles, days and altitude bands.
fn wind(lon: f64, lat: f64, pressure: f32, hour: f64) -> (f32, f32, f32) {
    let (l, f) = (lon.to_radians(), lat.to_radians());
    let aloft = 1.0 + f64::from(100_000.0 - pressure) / 50_000.0;
    let pulse = 1.0 + 0.2 * (hour / 12.0).sin();
    let u = (18.0 * f.cos() + 6.0 * (2.0 * l).sin() * f.cos()) * aloft * pulse;
    let v = 7.0 * (3.0 * l + hour / 20.0).sin() * f.cos() * aloft;
    let w = 0.25 * (2.0 * l).cos() * f.cos();
    (u as f32, v as f32, w as f32)
}

pub fn run(data: &Path) -> Result<()> {
    let entries: Vec<Entry> = (0..=HOURS)
        .step_by(3)
        .map(|forecast_hour| Entry {
            cycle: CYCLE,
            forecast_hour,
        })
        .collect();
    fs::create_dir_all(timeline::wind_dir(data))?;
    let days = timeline::group_days(&entries);
    let failure = Mutex::new(None);
    std::thread::scope(|scope| {
        for day in &days {
            scope.spawn(|| {
                let fields: Vec<_> = day
                    .entries
                    .iter()
                    .map(|entry| {
                        let hour = f64::from(entry.forecast_hour);
                        GlobalFields::from_fn(
                            entry.valid_time(),
                            |lon, lat, p| wind(lon, lat, p, hour),
                            |lon, lat| {
                                let pressure = 100_500.0 + 800.0 * lon.to_radians().sin() as f32;
                                (pressure, 300.0 - 0.5 * lat.abs() as f32)
                            },
                        )
                        .quantize()
                    })
                    .collect();
                if let Err(e) = pack::write_day(data, day, &fields) {
                    *failure.lock().unwrap() = Some(e.to_string());
                }
            });
        }
    });
    if let Some(message) = failure.into_inner().unwrap() {
        return Err(message.into());
    }
    timeline::write_manifest(data, &timeline::published(data)?, CYCLE)?;
    Ok(())
}
