//! Cuts global wind fields into tiles.

use crate::codec::{quantize, quantize_ps, Tile, OMEGA_SCALE, WIND_SCALE};
use crate::grid::{global_index, TileId, CELL_DEG, NLAT, NLON, PLANE, TILE_COUNT, TILE_N};

/// Wind on the whole 1° grid at one valid time. Three-dimensional fields are
/// stored level by level, each level row by row from 90°N, each row from 0°E.
pub struct GlobalFields {
    pub valid_time: i64,
    /// Pressure levels in pascals, highest pressure first.
    pub levels_pa: Vec<f32>,
    pub u: Vec<f32>,
    pub v: Vec<f32>,
    pub w: Vec<f32>,
    /// Surface pressure in pascals, `NLAT × NLON`.
    pub ps: Vec<f32>,
}

impl GlobalFields {
    /// Fills the grid from functions of `(lon, lat, pressure_pa)` returning
    /// `(u, v, omega)` and of `(lon, lat)` returning surface pressure.
    pub fn from_fn(
        valid_time: i64,
        levels_pa: &[f32],
        wind: impl Fn(f64, f64, f32) -> (f32, f32, f32),
        surface: impl Fn(f64, f64) -> f32,
    ) -> Self {
        let n = levels_pa.len() * NLAT * NLON;
        let (mut u, mut v, mut w) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        let coords = |index: usize| {
            let (row, col) = (index / NLON, index % NLON);
            (col as f64 * CELL_DEG, 90.0 - row as f64 * CELL_DEG)
        };
        for level in levels_pa {
            for index in 0..NLAT * NLON {
                let (lon, lat) = coords(index);
                let (a, b, c) = wind(lon, lat, *level);
                u.push(a);
                v.push(b);
                w.push(c);
            }
        }
        let ps = (0..NLAT * NLON)
            .map(|index| {
                let (lon, lat) = coords(index);
                surface(lon, lat)
            })
            .collect();
        Self {
            valid_time,
            levels_pa: levels_pa.to_vec(),
            u,
            v,
            w,
            ps,
        }
    }

    /// Cuts out one tile.
    pub fn tile(&self, tile: TileId) -> Tile {
        let levels = self.levels_pa.len();
        let cut =
            |field: &[f32], level: usize, convert: &dyn Fn(f32) -> i16, out: &mut Vec<i16>| {
                let base = level * NLAT * NLON;
                for j in 0..TILE_N {
                    for i in 0..TILE_N {
                        let (col, row) = global_index(tile, i, j);
                        out.push(convert(field[base + row * NLON + col]));
                    }
                }
            };
        let wind = |x: f32| quantize(x, WIND_SCALE);
        let omega = |x: f32| quantize(x, OMEGA_SCALE);
        let (mut u, mut v, mut w, mut ps) = (
            Vec::with_capacity(levels * PLANE),
            Vec::with_capacity(levels * PLANE),
            Vec::with_capacity(levels * PLANE),
            Vec::with_capacity(PLANE),
        );
        for level in 0..levels {
            cut(&self.u, level, &wind, &mut u);
            cut(&self.v, level, &wind, &mut v);
            cut(&self.w, level, &omega, &mut w);
        }
        cut(&self.ps, 0, &quantize_ps, &mut ps);
        Tile {
            valid_time: self.valid_time,
            tile,
            levels_pa: self.levels_pa.clone(),
            wind_scale: WIND_SCALE,
            omega_scale: OMEGA_SCALE,
            u,
            v,
            w,
            ps,
        }
    }

    /// Cuts out every tile.
    pub fn tiles(&self) -> impl Iterator<Item = Tile> + '_ {
        (0..TILE_COUNT).map(|t| self.tile(t as TileId))
    }
}
