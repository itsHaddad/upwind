//! The set of wind tiles loaded so far, and interpolation inside it.

use crate::codec::{Tile, PS_OFFSET, PS_SCALE};
use crate::grid::{locate, TileId, TilePos, PLANE, TILE_COUNT, TILE_N};

/// A tile the store does not hold yet: `(valid_time, tile)`.
pub type TileKey = (i64, TileId);

/// Why a point could not be sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Miss {
    /// The tile is needed and has not been added.
    Tile(TileKey),
    /// The time is outside the store's timeline.
    OutOfTime,
}

/// Wind and surface pressure at one point and time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Eastward wind, metres per second.
    pub u: f32,
    /// Northward wind, metres per second.
    pub v: f32,
    /// Pressure vertical velocity, pascals per second (positive is sinking).
    pub w: f32,
    /// Surface pressure, pascals.
    pub ps: f32,
}

impl Sample {
    fn mix(a: Self, b: Self, t: f32) -> Self {
        let l = |x: f32, y: f32| x + (y - x) * t;
        Self {
            u: l(a.u, b.u),
            v: l(a.v, b.v),
            w: l(a.w, b.w),
            ps: l(a.ps, b.ps),
        }
    }
}

/// Tiles indexed by position on a fixed timeline.
pub struct WindStore {
    times: Vec<i64>,
    tiles: Vec<Option<Box<Tile>>>,
}

impl WindStore {
    /// Creates an empty store for the given valid times (unix seconds).
    pub fn new(mut times: Vec<i64>) -> Self {
        times.sort_unstable();
        times.dedup();
        let slots = times.len() * TILE_COUNT;
        Self {
            times,
            tiles: (0..slots).map(|_| None).collect(),
        }
    }

    pub fn times(&self) -> &[i64] {
        &self.times
    }

    /// Adds a tile. Returns false when its time is not on the timeline.
    pub fn add(&mut self, tile: Tile) -> bool {
        match self.times.binary_search(&tile.valid_time) {
            Ok(t) => {
                let slot = t * TILE_COUNT + tile.tile as usize;
                self.tiles[slot] = Some(Box::new(tile));
                true
            }
            Err(_) => false,
        }
    }

    pub fn has(&self, key: TileKey) -> bool {
        self.times
            .binary_search(&key.0)
            .is_ok_and(|t| self.tiles[t * TILE_COUNT + key.1 as usize].is_some())
    }

    /// Number of tiles held.
    pub fn len(&self) -> usize {
        self.tiles.iter().filter(|t| t.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Interpolates wind at a point: bilinear across the cell, linear in
    /// pressure between levels and linear in time between valid times.
    pub fn sample(&self, lon: f64, lat: f64, pressure: f64, time: f64) -> Result<Sample, Miss> {
        let (first, last) = match (self.times.first(), self.times.last()) {
            (Some(a), Some(b)) => (*a as f64, *b as f64),
            _ => return Err(Miss::OutOfTime),
        };
        if !(first..=last).contains(&time) {
            return Err(Miss::OutOfTime);
        }
        let after = self.times.partition_point(|t| (*t as f64) <= time);
        let t0 = after.saturating_sub(1);
        let t1 = after.min(self.times.len() - 1);
        let pos = locate(lon, lat);
        let a = self.sample_at(t0, &pos, pressure as f32)?;
        if t1 == t0 {
            return Ok(a);
        }
        let b = self.sample_at(t1, &pos, pressure as f32)?;
        let span = (self.times[t1] - self.times[t0]) as f64;
        let frac = ((time - self.times[t0] as f64) / span) as f32;
        Ok(Sample::mix(a, b, frac))
    }

    fn sample_at(&self, t: usize, pos: &TilePos, pressure: f32) -> Result<Sample, Miss> {
        let tile = self.tiles[t * TILE_COUNT + pos.tile as usize]
            .as_deref()
            .ok_or(Miss::Tile((self.times[t], pos.tile)))?;
        let (k0, k1, az) = bracket(&tile.levels_pa, pressure);
        let cell = pos.j * TILE_N + pos.i;
        let corner = |field: &[i16], k: usize| -> f32 {
            let o = k * PLANE + cell;
            let north = lerp(field[o] as f32, field[o + 1] as f32, pos.ax);
            let south = lerp(
                field[o + TILE_N] as f32,
                field[o + TILE_N + 1] as f32,
                pos.ax,
            );
            lerp(north, south, pos.ay)
        };
        let column = |field: &[i16]| -> f32 {
            let low = corner(field, k0);
            if k1 == k0 {
                low
            } else {
                lerp(low, corner(field, k1), az)
            }
        };
        Ok(Sample {
            u: column(&tile.u) * tile.wind_scale,
            v: column(&tile.v) * tile.wind_scale,
            w: column(&tile.w) * tile.omega_scale,
            ps: corner(&tile.ps, 0) * PS_SCALE + PS_OFFSET,
        })
    }

    /// Lowest pressure (highest altitude) the loaded data reaches, pascals.
    pub fn top_pressure(&self) -> Option<f32> {
        self.tiles
            .iter()
            .flatten()
            .next()
            .and_then(|t| t.levels_pa.last().copied())
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Finds the two levels around `pressure` and the fraction between them.
/// Levels run from highest pressure to lowest; outside them the nearest
/// level is used.
fn bracket(levels: &[f32], pressure: f32) -> (usize, usize, f32) {
    let last = levels.len() - 1;
    if pressure >= levels[0] {
        return (0, 0, 0.0);
    }
    if pressure <= levels[last] {
        return (last, last, 0.0);
    }
    let k = levels.partition_point(|l| *l >= pressure) - 1;
    let frac = (levels[k] - pressure) / (levels[k] - levels[k + 1]);
    (k, k + 1, frac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_interpolates_and_clamps() {
        let levels = [100_000.0, 90_000.0, 70_000.0];
        assert_eq!(bracket(&levels, 101_000.0), (0, 0, 0.0));
        assert_eq!(bracket(&levels, 60_000.0), (2, 2, 0.0));
        let (k0, k1, f) = bracket(&levels, 80_000.0);
        assert_eq!((k0, k1), (1, 2));
        assert!((f - 0.5).abs() < 1e-6);
        assert_eq!(bracket(&levels, 90_000.0), (1, 2, 0.0));
    }
}
