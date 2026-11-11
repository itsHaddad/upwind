//! Downloaded wind, kept on disk between runs so that packing a day does
//! not download it again. One file per valid time, the whole globe.

use std::fs;
use std::path::Path;

use upwind_core::build::{GlobalCounts, GLOBAL_PLANE};
use upwind_core::grid::LEVELS_PA;

use crate::gfs::Result;

const MAGIC: &[u8; 4] = b"UWG2";

fn to_bytes(values: &[i16], out: &mut Vec<u8>) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

pub fn write(path: &Path, counts: &GlobalCounts) -> Result<()> {
    let mut raw = Vec::with_capacity((3 * LEVELS_PA.len() + 2) * GLOBAL_PLANE * 2);
    for field in [&counts.u, &counts.v, &counts.w, &counts.ps, &counts.t2] {
        to_bytes(field, &mut raw);
    }
    let mut bytes = Vec::from(MAGIC);
    bytes.extend_from_slice(&counts.valid_time.to_le_bytes());
    bytes.extend(miniz_oxide::deflate::compress_to_vec(&raw, 1));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("partial");
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}

pub fn read(path: &Path) -> Result<GlobalCounts> {
    let bytes = fs::read(path)?;
    if bytes.len() < 12 || &bytes[0..4] != MAGIC {
        return Err(format!("{} is not a cached wind field", path.display()).into());
    }
    let valid_time = i64::from_le_bytes(bytes[4..12].try_into().unwrap());
    let n = LEVELS_PA.len() * GLOBAL_PLANE;
    let expected = (3 * n + 2 * GLOBAL_PLANE) * 2;
    let raw = miniz_oxide::inflate::decompress_to_vec_with_limit(&bytes[12..], expected)
        .ok()
        .filter(|raw| raw.len() == expected)
        .ok_or_else(|| format!("{} is damaged", path.display()))?;
    let mut values = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair));
    let mut take = |count: usize| values.by_ref().take(count).collect::<Vec<i16>>();
    Ok(GlobalCounts {
        valid_time,
        u: take(n),
        v: take(n),
        w: take(n),
        ps: take(GLOBAL_PLANE),
        t2: take(GLOBAL_PLANE),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cached_field_reads_back_the_same() {
        let n = LEVELS_PA.len() * GLOBAL_PLANE;
        let ramp = |seed: usize, count: usize| -> Vec<i16> {
            (0..count)
                .map(|k| ((k * seed) % 4001) as i16 - 2000)
                .collect()
        };
        let counts = GlobalCounts {
            valid_time: 1_794_398_400,
            u: ramp(3, n),
            v: ramp(7, n),
            w: ramp(11, n),
            ps: ramp(13, GLOBAL_PLANE),
            t2: ramp(17, GLOBAL_PLANE),
        };
        let dir = std::env::temp_dir().join(format!("upwind-cache-test-{}", std::process::id()));
        let path = dir.join("2026111112").join("000.uwg");
        write(&path, &counts).unwrap();
        assert_eq!(read(&path).unwrap(), counts);
        fs::write(&path, b"UWG2 not really").unwrap();
        assert!(read(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
