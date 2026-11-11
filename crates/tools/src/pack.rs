//! Packs a day of global wind into the files the page downloads.

use std::fs;
use std::path::Path;

use upwind_core::build::GlobalCounts;
use upwind_core::codec::encode;
use upwind_core::grid::{BandId, TileId, BAND_COUNT, TILE_COUNT};

use crate::gfs::Result;
use crate::timeline::{file_path, wind_dir, Day};

/// Deflate effort for published files.
const COMPRESSION: u8 = 8;

/// Writes one day's wind files from its fields (in time order). The day's
/// directory appears only once every file is written. Returns bytes written.
pub fn write_day(data: &Path, day: &Day, fields: &[GlobalCounts]) -> Result<u64> {
    let wind = wind_dir(data);
    let final_dir = wind.join(day.dir());
    let work_dir = wind.join(format!("{}.partial", day.dir()));
    let _ = fs::remove_dir_all(&work_dir);
    for band in 0..BAND_COUNT {
        fs::create_dir_all(work_dir.join(band.to_string()))?;
    }
    let mut written = 0;
    for tile in 0..TILE_COUNT as TileId {
        for band in 0..BAND_COUNT as BandId {
            let times: Vec<_> = fields.iter().map(|f| f.tile(tile, band)).collect();
            let bytes = encode(&times, COMPRESSION);
            written += bytes.len() as u64;
            fs::write(file_path(&work_dir, tile, band), bytes)?;
        }
    }
    let _ = fs::remove_dir_all(&final_dir);
    fs::rename(&work_dir, &final_dir)?;
    Ok(written)
}
