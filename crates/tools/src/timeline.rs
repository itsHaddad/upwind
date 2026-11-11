//! Which model output makes up the wind history, and where it is on disk.
//!
//! The history is one valid time every three hours. Past cycles contribute
//! their analysis and three-hour forecast; the newest cycle also contributes
//! a forecast tail so the history reaches the present.
//!
//! Downloaded fields are cached per valid time under `cache/`. What the page
//! reads is under `wind/`: one directory per UTC day, holding one file per
//! tile and altitude band. A day's directory name includes a fingerprint of
//! the model output it was built from, so a file never changes once
//! published and can be cached for good.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use upwind_core::codec::{read_header, FORMAT};
use upwind_core::grid::{BandId, TileId, LEVELS_PA};

use crate::time::{cycle_to_unix, unix_to_cycle};

pub const CYCLE_S: i64 = 21_600;
pub const DAY_S: i64 = 86_400;
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

    /// `2026111112/003`, for messages.
    pub fn name(&self) -> String {
        format!("{}/{:03}", unix_to_cycle(self.cycle), self.forecast_hour)
    }

    /// Where this entry's downloaded field is cached.
    pub fn cache_path(&self, data: &Path) -> PathBuf {
        data.join("cache").join(format!("{}.uwg", self.name()))
    }
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

/// The valid times of one UTC day and the model output behind them.
#[derive(Debug, Clone, PartialEq)]
pub struct Day {
    /// Unix seconds of the day's midnight.
    pub start: i64,
    pub entries: Vec<Entry>,
}

impl Day {
    /// Directory of this day's wind files, relative to `wind/`: the date and
    /// a fingerprint of the file format and the entries.
    pub fn dir(&self) -> String {
        // FNV-1a over the entries.
        let mut hash: u32 = (0x811c_9dc5 ^ u32::from(FORMAT)).wrapping_mul(0x0100_0193);
        for entry in &self.entries {
            let words = [entry.cycle as u64, u64::from(entry.forecast_hour)];
            for byte in words.iter().flat_map(|w| w.to_le_bytes()) {
                hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
            }
        }
        format!("{}.{hash:08x}", &unix_to_cycle(self.start)[..8])
    }

    #[cfg(test)]
    pub fn times(&self) -> Vec<i64> {
        self.entries.iter().map(Entry::valid_time).collect()
    }
}

/// Groups entries (in time order) by the UTC day of their valid time.
pub fn group_days(entries: &[Entry]) -> Vec<Day> {
    let mut days: Vec<Day> = Vec::new();
    for entry in entries {
        let start = entry.valid_time().div_euclid(DAY_S) * DAY_S;
        match days.last_mut() {
            Some(day) if day.start == start => day.entries.push(*entry),
            _ => days.push(Day {
                start,
                entries: vec![*entry],
            }),
        }
    }
    days
}

pub fn wind_dir(data: &Path) -> PathBuf {
    data.join("wind")
}

/// Path of a wind file inside a day's directory.
pub fn file_path(day_dir: &Path, tile: TileId, band: BandId) -> PathBuf {
    day_dir.join(band.to_string()).join(format!("{tile}.uwb"))
}

/// A day of wind files found on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Published {
    pub dir: String,
    pub times: Vec<i64>,
}

/// The days published under `wind/`, in time order. Their valid times are
/// read from the first file of each.
pub fn published(data: &Path) -> io::Result<Vec<Published>> {
    let wind = wind_dir(data);
    let mut days = Vec::new();
    for item in fs::read_dir(&wind)? {
        let item = item?;
        let Some(dir) = item.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let dated = dir.len() > 9 && cycle_to_unix(&format!("{}00", &dir[..8])).is_some();
        if !dated || dir.ends_with(".partial") {
            continue;
        }
        let Ok(bytes) = fs::read(file_path(&item.path(), 0, 0)) else {
            continue;
        };
        let header =
            read_header(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        days.push(Published {
            dir,
            times: header.valid_times,
        });
    }
    days.sort_by_key(|day| day.times.first().copied());
    Ok(days)
}

/// The manifest the web page reads to learn what history exists.
pub fn manifest_json(days: &[Published], generated: i64) -> String {
    let levels: Vec<String> = LEVELS_PA.iter().map(|p| format!("{}", p / 100.0)).collect();
    let listed: Vec<String> = days
        .iter()
        .map(|day| {
            let times: Vec<String> = day.times.iter().map(i64::to_string).collect();
            format!(
                "{{\"dir\":\"{}\",\"times\":[{}]}}",
                day.dir,
                times.join(",")
            )
        })
        .collect();
    format!(
        "{{\"version\":2,\"generated\":{generated},\"step_hours\":{STEP_HOURS},\
         \"levels_hpa\":[{}],\"days\":[{}]}}\n",
        levels.join(","),
        listed.join(",")
    )
}

pub fn write_manifest(data: &Path, days: &[Published], generated: i64) -> io::Result<()> {
    let wind = wind_dir(data);
    let temporary = wind.join("manifest.json.partial");
    fs::write(&temporary, manifest_json(days, generated))?;
    fs::rename(temporary, wind.join("manifest.json"))
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
        assert_eq!(entries.last().unwrap().name(), "2026111112/012");
    }

    #[test]
    fn days_group_by_utc_date_and_are_named_by_their_sources() {
        let latest = cycle_to_unix("2026111112").unwrap();
        let days = group_days(&plan(latest, 2));
        assert_eq!(days.len(), 4);
        assert_eq!(days[0].times().len(), 4, "the first day starts at noon");
        assert_eq!(days[1].times().len(), 8);
        assert_eq!(days[3].times(), vec![latest + 12 * 3600]);
        assert!(days[1].dir().starts_with("20261110."));

        // The same day built from a newer run gets a new name.
        let later = group_days(&plan(latest + CYCLE_S, 2));
        let today = |days: &[Day]| {
            days.iter()
                .find(|d| d.dir().starts_with("20261111."))
                .cloned()
        };
        assert_ne!(today(&days).unwrap().dir(), today(&later).unwrap().dir());
        let yesterday = |days: &[Day]| {
            days.iter()
                .find(|d| d.dir().starts_with("20261110."))
                .cloned()
        };
        assert_eq!(
            yesterday(&days).unwrap().dir(),
            yesterday(&later).unwrap().dir()
        );
    }

    #[test]
    fn manifest_lists_days_and_times() {
        let day = Published {
            dir: "20261111.0badcafe".to_string(),
            times: vec![1_794_355_200, 1_794_366_000],
        };
        let json = manifest_json(&[day], 5);
        assert!(
            json.contains(
                "\"days\":[{\"dir\":\"20261111.0badcafe\",\"times\":[1794355200,1794366000]}]"
            ),
            "{json}"
        );
        assert!(json.contains("\"levels_hpa\":[1000,975,"));
    }
}
