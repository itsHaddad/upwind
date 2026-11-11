//! Reads the wind out of NOAA's GFS model output.
//!
//! Each forecast hour of a cycle is one GRIB2 file with several hundred
//! fields, next to a small index that lists the byte offset of every field.
//! Only the fields a trajectory needs are downloaded, by byte range.

use std::error::Error;
use std::time::Duration;

use grib::LatLons;
use upwind_core::build::GlobalFields;
use upwind_core::grid::{LEVELS_PA, NLAT, NLON};

use crate::time::{cycle_to_unix, unix_to_cycle};

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const BUCKET: &str = "https://noaa-gfs-bdp-pds.s3.amazonaws.com";

/// Neighbouring fields closer than this are fetched in one request.
const MERGE_GAP: u64 = 250_000;
const ATTEMPTS: u32 = 4;

/// One field's position in a GRIB file.
#[derive(Debug, Clone, PartialEq)]
struct IndexEntry {
    start: u64,
    /// One past the last byte; `None` for the last field in the file.
    end: Option<u64>,
    variable: String,
    level: String,
}

pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(90)))
        .build()
        .into()
}

fn file_url(cycle_unix: i64, forecast_hour: u32) -> String {
    let cycle = unix_to_cycle(cycle_unix);
    let (date, hour) = cycle.split_at(8);
    format!("{BUCKET}/gfs.{date}/{hour}/atmos/gfs.t{hour}z.pgrb2.1p00.f{forecast_hour:03}")
}

/// Whether a forecast hour of a cycle has been published.
pub fn is_published(agent: &ureq::Agent, cycle_unix: i64, forecast_hour: u32) -> bool {
    let url = format!("{}.idx", file_url(cycle_unix, forecast_hour));
    agent.head(&url).call().is_ok()
}

fn parse_index(text: &str) -> Vec<IndexEntry> {
    let mut entries: Vec<IndexEntry> = Vec::new();
    for line in text.lines() {
        let mut parts = line.split(':');
        let (Some(_), Some(offset), Some(_), Some(variable), Some(level)) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            continue;
        };
        let Ok(start) = offset.parse::<u64>() else {
            continue;
        };
        if let Some(previous) = entries.last_mut() {
            // Several fields can share one message; they share its offset.
            if previous.start != start {
                previous.end = Some(start);
            }
        }
        entries.push(IndexEntry {
            start,
            end: None,
            variable: variable.to_string(),
            level: level.to_string(),
        });
    }
    // Entries that share an offset with their successor get its end.
    for i in (0..entries.len().saturating_sub(1)).rev() {
        if entries[i].end.is_none() && entries[i].start == entries[i + 1].start {
            entries[i].end = entries[i + 1].end;
        }
    }
    entries
}

fn retry<T>(what: &str, mut attempt: impl FnMut() -> Result<T>) -> Result<T> {
    let mut wait = Duration::from_millis(500);
    for round in 1..=ATTEMPTS {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(e) if round == ATTEMPTS => return Err(format!("{what}: {e}").into()),
            Err(_) => {
                std::thread::sleep(wait);
                wait *= 3;
            }
        }
    }
    unreachable!()
}

fn fetch_range(agent: &ureq::Agent, url: &str, start: u64, end: u64) -> Result<Vec<u8>> {
    retry(url, || {
        let mut response = agent
            .get(url)
            .header("Range", &format!("bytes={start}-{}", end - 1))
            .call()?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(64 << 20)
            .read_to_vec()?;
        if bytes.len() as u64 != end - start {
            return Err(format!("got {} of {} bytes", bytes.len(), end - start).into());
        }
        Ok(bytes)
    })
}

/// Decodes one GRIB2 message into its grid values.
fn decode(message: &[u8]) -> Result<Vec<f32>> {
    let grib = grib::from_bytes(message)?;
    let (_, submessage) = grib.iter().next().ok_or("empty GRIB message")?;
    let mut latlons = submessage.latlons()?;
    let first = latlons.next().ok_or("GRIB message has no grid")?;
    if (first.0 - 90.0).abs() > 1e-3 || first.1.abs() > 1e-3 {
        return Err(format!("grid starts at {first:?}, expected 90N 0E").into());
    }
    let decoder = grib::Grib2SubmessageDecoder::from(submessage)?;
    let values: Vec<f32> = decoder.dispatch()?.collect();
    if values.len() != NLAT * NLON {
        return Err(format!("grid has {} points, expected {}", values.len(), NLAT * NLON).into());
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Err("field has missing values".into());
    }
    Ok(values)
}

/// Downloads the wind of one forecast hour of one cycle.
pub fn fetch_fields(
    agent: &ureq::Agent,
    cycle_unix: i64,
    forecast_hour: u32,
) -> Result<GlobalFields> {
    let url = file_url(cycle_unix, forecast_hour);
    let index_text = retry(&url, || {
        Ok(agent
            .get(format!("{url}.idx"))
            .call()?
            .body_mut()
            .read_to_string()?)
    })?;
    let index = parse_index(&index_text);

    // What is wanted, as (slot, entry). Slots: variable × level, then the
    // two surface fields.
    let levels = LEVELS_PA.len();
    let mut wanted: Vec<(usize, &IndexEntry)> = Vec::new();
    for (v, variable) in ["UGRD", "VGRD", "VVEL"].iter().enumerate() {
        for (k, level) in LEVELS_PA.iter().enumerate() {
            let name = format!("{} mb", (level / 100.0).round() as u32);
            let entry = index
                .iter()
                .find(|e| e.variable == *variable && e.level == name)
                .ok_or_else(|| format!("{variable} at {name} is not in {url}"))?;
            wanted.push((v * levels + k, entry));
        }
    }
    let surface = index
        .iter()
        .find(|e| e.variable == "PRES" && e.level == "surface")
        .ok_or_else(|| format!("surface pressure is not in {url}"))?;
    wanted.push((3 * levels, surface));
    let temperature = index
        .iter()
        .find(|e| e.variable == "TMP" && e.level == "2 m above ground")
        .ok_or_else(|| format!("2 m temperature is not in {url}"))?;
    wanted.push((3 * levels + 1, temperature));
    wanted.sort_by_key(|(_, e)| e.start);

    let mut fields: Vec<Option<Vec<f32>>> = vec![None; 3 * levels + 2];
    let mut group_start = 0;
    while group_start < wanted.len() {
        let mut group_end = group_start + 1;
        while group_end < wanted.len() {
            let previous_end = wanted[group_end - 1].1.end.ok_or("field without an end")?;
            if wanted[group_end].1.start > previous_end + MERGE_GAP {
                break;
            }
            group_end += 1;
        }
        let from = wanted[group_start].1.start;
        let to = wanted[group_end - 1].1.end.ok_or("field without an end")?;
        let bytes = fetch_range(agent, &url, from, to)?;
        for (slot, entry) in &wanted[group_start..group_end] {
            let end = entry.end.ok_or("field without an end")?;
            let message = &bytes[(entry.start - from) as usize..(end - from) as usize];
            let values = decode(message)
                .map_err(|e| format!("{} {} in {url}: {e}", entry.variable, entry.level))?;
            fields[*slot] = Some(values);
        }
        group_start = group_end;
    }

    let mut take = |range: std::ops::Range<usize>| -> Vec<f32> {
        range
            .flat_map(|slot| fields[slot].take().unwrap())
            .collect()
    };
    Ok(GlobalFields {
        valid_time: cycle_unix + i64::from(forecast_hour) * 3600,
        u: take(0..levels),
        v: take(levels..2 * levels),
        w: take(2 * levels..3 * levels),
        ps: take(3 * levels..3 * levels + 1),
        t2: take(3 * levels + 1..3 * levels + 2),
    })
}

/// The newest cycle at or before `now` whose forecast hour `through` exists.
pub fn latest_cycle(agent: &ureq::Agent, now: i64, through: u32) -> Result<i64> {
    let newest = now.div_euclid(21_600) * 21_600;
    (0..6)
        .map(|back| newest - back * 21_600)
        .find(|cycle| is_published(agent, *cycle, through))
        .ok_or_else(|| "no GFS cycle from the last 36 hours is published".into())
}

/// Checks that a cycle string is well formed; used by the command line.
pub fn parse_cycle(text: &str) -> Result<i64> {
    cycle_to_unix(text).ok_or_else(|| format!("'{text}' is not a cycle like 2026111112").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_gives_every_field_its_byte_range() {
        let text = "1:0:d=2026111112:PRMSL:mean sea level:anl:\n\
                    2:1000:d=2026111112:UGRD:850 mb:anl:\n\
                    3:1000:d=2026111112:VGRD:850 mb:anl:\n\
                    4:2500:d=2026111112:PRES:surface:anl:\n";
        let index = parse_index(text);
        assert_eq!(index.len(), 4);
        assert_eq!((index[0].start, index[0].end), (0, Some(1000)));
        assert_eq!((index[1].start, index[1].end), (1000, Some(2500)));
        assert_eq!((index[2].start, index[2].end), (1000, Some(2500)));
        assert_eq!(index[3].end, None);
        assert_eq!(index[2].variable, "VGRD");
        assert_eq!(index[2].level, "850 mb");
    }

    #[test]
    fn file_urls_follow_the_bucket_layout() {
        let cycle = cycle_to_unix("2026111112").unwrap();
        assert_eq!(
            file_url(cycle, 3),
            "https://noaa-gfs-bdp-pds.s3.amazonaws.com/gfs.20261111/12/atmos/gfs.t12z.pgrb2.1p00.f003"
        );
    }
}
