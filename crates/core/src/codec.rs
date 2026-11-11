//! Binary format of a wind file: one tile and altitude band at the valid
//! times of one day.
//!
//! Layout (little endian):
//!
//! ```text
//!  0  magic "UWB4"
//!  4  u16  tile id
//!  6  u8   altitude band
//!  7  u8   number of pressure levels (L)
//!  8  u8   points per tile edge (16)
//!  9  u8   number of valid times (T)
//! 10  u16  reserved, 0
//! 12  f32  wind scale: metres per second per count
//! 16  f32  omega scale: pascals per second per count
//! 20  L×f32 pressure levels in pascals, highest pressure first
//!     T×i64 valid times, unix seconds
//!     ...  deflate stream of the planes
//! ```
//!
//! For every valid time the planes are u, v and omega for every level, then
//! surface pressure and the temperature two metres above the ground, all as
//! 16-bit counts. Each plane is predicted from its
//! left (or upper) neighbour, the residuals are zigzag coded, and low and
//! high bytes are stored apart so that deflate sees long runs of similar
//! bytes.

use crate::grid::{band_levels, BandId, TileId, BAND_COUNT, PLANE, TILE_COUNT, TILE_N};

/// Changes whenever the files change shape; also part of every published
/// directory name, so old and new files never mix.
pub const FORMAT: u8 = 4;
const MAGIC: &[u8; 4] = b"UWB4";
const HEADER: usize = 20;
/// Most valid times one file may hold.
const MAX_TIMES: usize = 32;

/// Metres per second per stored count.
pub const WIND_SCALE: f32 = 0.1;
/// Pascals per second per stored count.
pub const OMEGA_SCALE: f32 = 0.01;
/// Surface pressure is stored as `(pascals - PS_OFFSET) / PS_SCALE`.
pub const PS_SCALE: f32 = 10.0;
pub const PS_OFFSET: f32 = 50_000.0;
/// Temperature is stored as `(kelvin - T_OFFSET) / T_SCALE`.
pub const T_SCALE: f32 = 0.02;
pub const T_OFFSET: f32 = 273.15;

/// Wind in one tile and band at one time, as stored counts.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    pub valid_time: i64,
    pub tile: TileId,
    pub band: BandId,
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
    /// Temperature two metres above the ground, `PLANE` counts.
    pub t2: Vec<i16>,
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
            Self::TooShort => "wind file is shorter than its header",
            Self::BadMagic => "not a wind file",
            Self::BadShape => "wind file has an unsupported shape",
            Self::BadPayload => "wind file payload is corrupt",
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

/// Converts a temperature in kelvin to a stored count.
pub fn quantize_t(kelvin: f32) -> i16 {
    quantize(kelvin - T_OFFSET, T_SCALE)
}

impl Tile {
    pub fn levels(&self) -> usize {
        self.levels_pa.len()
    }

    /// Counts in this tile: three fields on every level and two at the
    /// surface.
    fn counts(&self) -> usize {
        3 * self.levels() * PLANE + 2 * PLANE
    }
}

/// Serializes the valid times of one tile and band into one file. `level`
/// is the deflate effort, 1 (fast) to 10.
///
/// # Panics
/// When `tiles` is empty, or its members differ in anything but time.
pub fn encode(tiles: &[Tile], level: u8) -> Vec<u8> {
    let first = tiles.first().expect("a wind file holds at least one time");
    assert!(tiles.len() <= MAX_TIMES);
    let n = first.levels() * PLANE;
    let mut residuals = Vec::with_capacity(tiles.len() * first.counts());
    for tile in tiles {
        assert!(
            (tile.tile, tile.band, &tile.levels_pa) == (first.tile, first.band, &first.levels_pa)
                && (tile.wind_scale, tile.omega_scale) == (first.wind_scale, first.omega_scale),
            "all times in a wind file share tile, band, levels and scales"
        );
        assert!(tile.u.len() == n && tile.v.len() == n && tile.w.len() == n);
        assert!(tile.ps.len() == PLANE && tile.t2.len() == PLANE);
        for field in [&tile.u, &tile.v, &tile.w, &tile.ps, &tile.t2] {
            for plane in field.as_chunks::<PLANE>().0 {
                predict(plane, &mut residuals);
            }
        }
    }
    let mut shuffled = Vec::with_capacity(residuals.len() * 2);
    shuffled.extend(residuals.iter().map(|r| *r as u8));
    shuffled.extend(residuals.iter().map(|r| (*r >> 8) as u8));

    let mut out = Vec::with_capacity(HEADER + shuffled.len() / 2);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&first.tile.to_le_bytes());
    out.extend_from_slice(&[
        first.band,
        first.levels() as u8,
        TILE_N as u8,
        tiles.len() as u8,
        0,
        0,
    ]);
    out.extend_from_slice(&first.wind_scale.to_le_bytes());
    out.extend_from_slice(&first.omega_scale.to_le_bytes());
    for p in &first.levels_pa {
        out.extend_from_slice(&p.to_le_bytes());
    }
    for tile in tiles {
        out.extend_from_slice(&tile.valid_time.to_le_bytes());
    }
    out.extend(miniz_oxide::deflate::compress_to_vec(&shuffled, level));
    out
}

/// What a wind file says about itself, without unpacking the wind.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub tile: TileId,
    pub band: BandId,
    pub levels_pa: Vec<f32>,
    pub wind_scale: f32,
    pub omega_scale: f32,
    pub valid_times: Vec<i64>,
    /// Offset of the deflate stream.
    body: usize,
}

/// Reads the header of a wind file.
pub fn read_header(bytes: &[u8]) -> Result<Header, DecodeError> {
    if bytes.len() < HEADER {
        return Err(DecodeError::TooShort);
    }
    if &bytes[0..4] != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let tile = TileId::from_le_bytes([bytes[4], bytes[5]]);
    let (band, levels, edge, times) = (
        bytes[6],
        bytes[7] as usize,
        bytes[8] as usize,
        bytes[9] as usize,
    );
    let shaped = edge == TILE_N
        && levels > 0
        && (1..=MAX_TIMES).contains(&times)
        && (tile as usize) < TILE_COUNT
        && (band as usize) < BAND_COUNT;
    if !shaped {
        return Err(DecodeError::BadShape);
    }
    let times_at = HEADER + 4 * levels;
    let body = times_at + 8 * times;
    if bytes.len() < body {
        return Err(DecodeError::TooShort);
    }
    let f32_at = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    // The store picks a file by band and then trusts its levels, so a file
    // whose levels are not its band's (or are not numbers) is refused here.
    let levels_pa: Vec<f32> = (0..levels).map(|k| f32_at(HEADER + 4 * k)).collect();
    if levels_pa != band_levels(band) {
        return Err(DecodeError::BadShape);
    }
    let i64_at = |o: usize| i64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
    Ok(Header {
        tile,
        band,
        levels_pa,
        wind_scale: f32_at(12),
        omega_scale: f32_at(16),
        valid_times: (0..times).map(|t| i64_at(times_at + 8 * t)).collect(),
        body,
    })
}

/// Parses a file written by [`encode`] into one tile per valid time.
pub fn decode(bytes: &[u8]) -> Result<Vec<Tile>, DecodeError> {
    let header = read_header(bytes)?;
    let n = header.levels_pa.len() * PLANE;
    let per_time = 3 * n + 2 * PLANE;
    let count = per_time * header.valid_times.len();
    let shuffled =
        miniz_oxide::inflate::decompress_to_vec_with_limit(&bytes[header.body..], count * 2)
            .map_err(|_| DecodeError::BadPayload)?;
    if shuffled.len() != count * 2 {
        return Err(DecodeError::BadPayload);
    }
    let (lo, hi) = shuffled.split_at(count);
    let mut tiles = Vec::with_capacity(header.valid_times.len());
    for (index, valid_time) in header.valid_times.iter().enumerate() {
        let base = index * per_time;
        let mut values = Vec::with_capacity(per_time);
        for start in (base..base + per_time).step_by(PLANE) {
            unpredict(
                &lo[start..start + PLANE],
                &hi[start..start + PLANE],
                &mut values,
            );
        }
        let t2 = values.split_off(3 * n + PLANE);
        let ps = values.split_off(3 * n);
        let w = values.split_off(2 * n);
        let v = values.split_off(n);
        tiles.push(Tile {
            valid_time: *valid_time,
            tile: header.tile,
            band: header.band,
            levels_pa: header.levels_pa.clone(),
            wind_scale: header.wind_scale,
            omega_scale: header.omega_scale,
            u: values,
            v,
            w,
            ps,
            t2,
        });
    }
    Ok(tiles)
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
