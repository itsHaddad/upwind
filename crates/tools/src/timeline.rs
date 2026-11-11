//! Which model output makes up the wind history, and where it is on disk.
//!
//! The history is one valid time every three hours. Past cycles contribute
//! their analysis and three-hour forecast; the newest cycle also contributes
//! a forecast tail so the history reaches the present.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use upwind_core::grid::TILE_COUNT;

use crate::gfs::LEVELS_HPA;
use crate::time::{cycle_to_unix, unix_to_cycle};

pub const CYCLE_S: i64 = 21_600;
/// Hours between valid times.
pub const STEP_HOURS: u32 = 3;
/// How far the newest cycle's forecast is used, in hours.
pub const TAIL_HOURS: u32 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub cycle: i64,
    pub forecast_hour: u32,
}

impl Entry {
    pub fn valid_time(&self) -> i64 {
        self.cycle + i64::from(self.forecast_hour) * 3600
    }

    /// Directory of this entry's tiles, relative to the data directory.
    pub fn dir(&self) -> String {
        format!("{}/{:03}", unix_to_cycle(self.cycle), self.forecast_hour)
    }
}

pub fn tile_path(data: &Path, entry: &Entry, tile: u8) -> PathBuf {
    data.join(entry.dir()).join(format!("{tile}.uwt"))
}

/// The entries that make up `days` days of history ending with `latest`.
pub fn plan(latest: i64, days: u32) -> Vec<Entry> {
    let cycles = i64::from(days) * 4;
    let mut entries = Vec::new();
    for back in (1..=cycles).rev() {
        for forecast_hour in (0..6).step_by(STEP_HOURS as usize) {
            entries.push(Entry {
                cycle: latest - back * CYCLE_S,
                forecast_hour,
            });
        }
    }
    for forecast_hour in (0..=TAIL_HOURS).step_by(STEP_HOURS as usize) {
        entries.push(Entry {
            cycle: latest,
            forecast_hour,
        });
    }
    entries
}

/// Whether every tile of an entry is on disk.
pub fn is_complete(data: &Path, entry: &Entry) -> bool {
    (0..TILE_COUNT).all(|tile| tile_path(data, entry, tile as u8).is_file())
}

/// Complete entries on disk, one per valid time (the newest cycle wins),
/// in time order.
pub fn scan(data: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for cycle_dir in fs::read_dir(data)? {
        let cycle_dir = cycle_dir?;
        let Some(cycle) = cycle_dir.file_name().to_str().and_then(cycle_to_unix) else {
            continue;
        };
        for hour_dir in fs::read_dir(cycle_dir.path())? {
            let name = hour_dir?.file_name();
            let Some(forecast_hour) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            let entry = Entry {
                cycle,
                forecast_hour,
            };
            if is_complete(data, &entry) {
                entries.push(entry);
            }
        }
    }
    entries.sort_by_key(|e| (e.valid_time(), std::cmp::Reverse(e.cycle)));
    entries.dedup_by_key(|e| e.valid_time());
    Ok(entries)
}

/// Removes tile directories that are not part of `keep`.
pub fn prune(data: &Path, keep: &[Entry]) -> io::Result<usize> {
    let mut removed = 0;
    for cycle_dir in fs::read_dir(data)? {
        let cycle_dir = cycle_dir?;
        let Some(cycle) = cycle_dir.file_name().to_str().and_then(cycle_to_unix) else {
            continue;
        };
        for hour_dir in fs::read_dir(cycle_dir.path())? {
            let hour_dir = hour_dir?;
            let forecast_hour = hour_dir
                .file_name()
                .to_str()
                .and_then(|n| n.parse::<u32>().ok());
            let kept = forecast_hour.is_some_and(|forecast_hour| {
                keep.contains(&Entry {
                    cycle,
                    forecast_hour,
                })
            });
            if !kept {
                fs::remove_dir_all(hour_dir.path())?;
                removed += 1;
            }
        }
        if fs::read_dir(cycle_dir.path())?.next().is_none() {
            fs::remove_dir(cycle_dir.path())?;
        }
    }
    Ok(removed)
}

/// The manifest the web page reads to learn what history exists.
pub fn manifest_json(entries: &[Entry], generated: i64) -> String {
    let levels: Vec<String> = LEVELS_HPA.iter().map(u32::to_string).collect();
    let times: Vec<String> = entries
        .iter()
        .map(|e| format!("[{},\"{}\"]", e.valid_time(), e.dir()))
        .collect();
    format!(
        "{{\"version\":1,\"generated\":{generated},\"step_hours\":{STEP_HOURS},\
         \"levels_hpa\":[{}],\"times\":[{}]}}\n",
        levels.join(","),
        times.join(",")
    )
}

pub fn write_manifest(data: &Path, entries: &[Entry], generated: i64) -> io::Result<()> {
    let temporary = data.join("manifest.json.tmp");
    fs::write(&temporary, manifest_json(entries, generated))?;
    fs::rename(temporary, data.join("manifest.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_is_one_valid_time_every_three_hours() {
        let latest = cycle_to_unix("2026111112").unwrap();
        let entries = plan(latest, 10);
        let times: Vec<i64> = entries.iter().map(Entry::valid_time).collect();
        assert_eq!(times[0], latest - 10 * 86_400);
        assert_eq!(*times.last().unwrap(), latest + 12 * 3600);
        assert!(times.windows(2).all(|w| w[1] - w[0] == 3 * 3600));
        assert_eq!(entries.last().unwrap().dir(), "2026111112/012");
    }

    #[test]
    fn manifest_lists_times_and_directories() {
        let entry = Entry {
            cycle: cycle_to_unix("2026111112").unwrap(),
            forecast_hour: 3,
        };
        let json = manifest_json(&[entry], 5);
        assert!(
            json.contains("\"times\":[[1794409200,\"2026111112/003\"]]"),
            "{json}"
        );
        assert!(json.contains("\"levels_hpa\":[1000,975,"));
    }
}
