//! Binary format of one wind tile at one valid time.
//!
//! Layout (little endian):
//!
//! ```text
//!  0  magic "UWT1"
//!  4  i64  valid time, unix seconds
//! 12  u8   tile id
//! 13  u8   number of pressure levels (L)
//! 14  u8   points per tile edge (31)
//! 15  u8   reserved, 0
//! 16  f32  wind scale: metres per second per count
//! 20  f32  omega scale: pascals per second per count
//! 24  L×f32 pressure levels in pascals, highest pressure first
//!     ...  deflate stream of the planes
//! ```
//!
//! The planes are u, v and omega for every level, then surface pressure, all
//! as 16-bit counts. Each plane is predicted from its left (or upper)
//! neighbour, the residuals are zigzag coded, and low and high bytes are
//! stored apart so that deflate sees long runs of similar bytes.

use crate::grid::{TileId, PLANE, TILE_N};

const MAGIC: &[u8; 4] = b"UWT1";
const HEADER: usize = 24;

/// Metres per second per stored count.
pub const WIND_SCALE: f32 = 0.05;
/// Pascals per second per stored count.
pub const OMEGA_SCALE: f32 = 0.002;
/// Surface pressure is stored as `(pascals - PS_OFFSET) / PS_SCALE`.
pub const PS_SCALE: f32 = 10.0;
pub const PS_OFFSET: f32 = 50_000.0;

/// Wind in one tile at one time, as stored counts.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    pub valid_time: i64,
    pub tile: TileId,
    /// Pressure levels in pascals, highest pressure (lowest altitude) first.
    pub levels_pa: Vec<f32>,
    pub wind_scale: f32,
    pub omega_scale: f32,
    /// Eastward wind, `levels × PLANE` counts.
    pub u: Vec<i16>,
    /// Northward wind, `levels × PLANE` counts.
    pub v: Vec<i16>,
    /// Pressure vertical velocity (positive down), `levels × PLANE` counts.
    pub w: Vec<i16>,
    /// Surface pressure, `PLANE` counts.
    pub ps: Vec<i16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    TooShort,
    BadMagic,
    BadShape,
    BadPayload,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::TooShort => "tile is shorter than its header",
            Self::BadMagic => "not a wind tile",
            Self::BadShape => "tile has an unsupported shape",
            Self::BadPayload => "tile payload is corrupt",
        })
    }
}

impl std::error::Error for DecodeError {}

/// Converts a physical value to a stored count, saturating at the i16 range.
pub fn quantize(value: f32, scale: f32) -> i16 {
    (value / scale)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

/// Converts surface pressure in pascals to a stored count.
pub fn quantize_ps(pascals: f32) -> i16 {
    quantize(pascals - PS_OFFSET, PS_SCALE)
}

impl Tile {
    pub fn levels(&self) -> usize {
        self.levels_pa.len()
    }

    /// Serializes the tile. `level` is the deflate effort, 1 (fast) to 10.
    pub fn encode(&self, level: u8) -> Vec<u8> {
        let n = self.levels() * PLANE;
        assert!(self.u.len() == n && self.v.len() == n && self.w.len() == n);
        assert_eq!(self.ps.len(), PLANE);

        let mut residuals = Vec::with_capacity(3 * n + PLANE);
        for field in [&self.u, &self.v, &self.w, &self.ps] {
            for plane in field.as_chunks::<PLANE>().0 {
                predict(plane, &mut residuals);
            }
        }
        let mut shuffled = Vec::with_capacity(residuals.len() * 2);
        shuffled.extend(residuals.iter().map(|r| *r as u8));
        shuffled.extend(residuals.iter().map(|r| (*r >> 8) as u8));

        let mut out = Vec::with_capacity(HEADER + shuffled.len() / 2);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.valid_time.to_le_bytes());
        out.extend_from_slice(&[self.tile, self.levels() as u8, TILE_N as u8, 0]);
        out.extend_from_slice(&self.wind_scale.to_le_bytes());
        out.extend_from_slice(&self.omega_scale.to_le_bytes());
        for p in &self.levels_pa {
            out.extend_from_slice(&p.to_le_bytes());
        }
        out.extend(miniz_oxide::deflate::compress_to_vec(&shuffled, level));
        out
    }

    /// Parses a tile written by [`Tile::encode`].
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() < HEADER {
            return Err(DecodeError::TooShort);
        }
        if &bytes[0..4] != MAGIC {
            return Err(DecodeError::BadMagic);
        }
        let valid_time = i64::from_le_bytes(bytes[4..12].try_into().unwrap());
        let (tile, levels, edge) = (bytes[12], bytes[13] as usize, bytes[14] as usize);
        if edge != TILE_N || levels == 0 || tile as usize >= crate::grid::TILE_COUNT {
            return Err(DecodeError::BadShape);
        }
        let f32_at = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let body = HEADER + 4 * levels;
        if bytes.len() < body {
            return Err(DecodeError::TooShort);
        }
        let levels_pa: Vec<f32> = (0..levels).map(|k| f32_at(HEADER + 4 * k)).collect();

        let n = levels * PLANE;
        let count = 3 * n + PLANE;
        let shuffled =
            miniz_oxide::inflate::decompress_to_vec_with_limit(&bytes[body..], count * 2)
                .map_err(|_| DecodeError::BadPayload)?;
        if shuffled.len() != count * 2 {
            return Err(DecodeError::BadPayload);
        }
        let (lo, hi) = shuffled.split_at(count);
        let mut values = Vec::with_capacity(count);
        for start in (0..count).step_by(PLANE) {
            unpredict(
                &lo[start..start + PLANE],
                &hi[start..start + PLANE],
                &mut values,
            );
        }
        let ps = values.split_off(3 * n);
        let w = values.split_off(2 * n);
        let v = values.split_off(n);
        Ok(Self {
            valid_time,
            tile,
            levels_pa,
            wind_scale: f32_at(16),
            omega_scale: f32_at(20),
            u: values,
            v,
            w,
            ps,
        })
    }
}

/// The value a point is predicted from: its west neighbour, or for the first
/// column the point above, or zero for the very first point.
fn predictor(done: &[i16], index: usize) -> i16 {
    if !index.is_multiple_of(TILE_N) {
        done[index - 1]
    } else if index >= TILE_N {
        done[index - TILE_N]
    } else {
        0
    }
}

fn predict(plane: &[i16], out: &mut Vec<u16>) {
    for (index, value) in plane.iter().enumerate() {
        let d = value.wrapping_sub(predictor(plane, index));
        out.push(((d << 1) ^ (d >> 15)) as u16);
    }
}

fn unpredict(lo: &[u8], hi: &[u8], out: &mut Vec<i16>) {
    let base = out.len();
    for index in 0..PLANE {
        let z = u16::from(lo[index]) | (u16::from(hi[index]) << 8);
        let d = ((z >> 1) as i16) ^ -((z & 1) as i16);
        let value = predictor(&out[base..], index).wrapping_add(d);
        out.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tile() -> Tile {
        let levels_pa = vec![100_000.0, 85_000.0, 50_000.0];
        let n = levels_pa.len() * PLANE;
        let wave = |k: usize, f: f32| quantize(40.0 * ((k as f32) * f).sin(), WIND_SCALE);
        Tile {
            valid_time: 1_794_000_000,
            tile: 17,
            levels_pa,
            wind_scale: WIND_SCALE,
            omega_scale: OMEGA_SCALE,
            u: (0..n).map(|k| wave(k, 0.013)).collect(),
            v: (0..n).map(|k| wave(k, 0.021)).collect(),
            w: (0..n)
                .map(|k| if k % 97 == 0 { i16::MIN } else { i16::MAX })
                .collect(),
            ps: (0..PLANE)
                .map(|k| quantize_ps(98_000.0 + k as f32))
                .collect(),
        }
    }

    #[test]
    fn round_trips_exactly() {
        let tile = sample_tile();
        let bytes = tile.encode(6);
        assert_eq!(Tile::decode(&bytes).unwrap(), tile);
    }

    #[test]
    fn smooth_fields_compress() {
        let mut tile = sample_tile();
        tile.w.fill(0);
        let raw = (3 * tile.u.len() + PLANE) * 2;
        assert!(tile.encode(6).len() * 3 < raw);
    }

    #[test]
    fn rejects_damaged_input() {
        let bytes = sample_tile().encode(6);
        assert_eq!(Tile::decode(&bytes[..10]), Err(DecodeError::TooShort));
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert_eq!(Tile::decode(&bad), Err(DecodeError::BadMagic));
        assert_eq!(
            Tile::decode(&bytes[..bytes.len() - 20]),
            Err(DecodeError::BadPayload)
        );
    }
}
