//! Cuts global wind fields into tiles.

use crate::codec::{quantize, quantize_ps, quantize_t, Tile, OMEGA_SCALE, WIND_SCALE};
use crate::grid::{
    edge_stride, global_index, BandId, TileId, BANDS, BAND_COUNT, CELL_DEG, LEVELS_PA, NLAT, NLON,
    PLANE, TILE_COUNT, TILE_N,
};

/// Points in one level of the global grid.
pub const GLOBAL_PLANE: usize = NLAT * NLON;

/// Wind on the whole 1° grid at one valid time, on every level of
/// [`LEVELS_PA`]. Three-dimensional fields are stored level by level, each
/// level row by row from 90°N, each row from 0°E.
pub struct GlobalFields {
    pub valid_time: i64,
    /// Eastward wind, metres per second.
    pub u: Vec<f32>,
    /// Northward wind, metres per second.
    pub v: Vec<f32>,
    /// Pressure vertical velocity, pascals per second.
    pub w: Vec<f32>,
    /// Surface pressure in pascals, one plane.
    pub ps: Vec<f32>,
    /// Temperature two metres above the ground in kelvin, one plane.
    pub t2: Vec<f32>,
}

/// The same as stored counts: what the wind files hold.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalCounts {
    pub valid_time: i64,
    pub u: Vec<i16>,
    pub v: Vec<i16>,
    pub w: Vec<i16>,
    pub ps: Vec<i16>,
    pub t2: Vec<i16>,
}

impl GlobalFields {
    /// Fills the grid from functions of `(lon, lat, pressure_pa)` returning
    /// `(u, v, omega)` and of `(lon, lat)` returning surface pressure and
    /// the temperature two metres above the ground.
    pub fn from_fn(
        valid_time: i64,
        wind: impl Fn(f64, f64, f32) -> (f32, f32, f32),
        surface: impl Fn(f64, f64) -> (f32, f32),
    ) -> Self {
        let n = LEVELS_PA.len() * GLOBAL_PLANE;
        let (mut u, mut v, mut w) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        let coords = |index: usize| {
            let (row, col) = (index / NLON, index % NLON);
            (col as f64 * CELL_DEG, 90.0 - row as f64 * CELL_DEG)
        };
        for level in LEVELS_PA {
            for index in 0..GLOBAL_PLANE {
                let (lon, lat) = coords(index);
                let (a, b, c) = wind(lon, lat, level);
                u.push(a);
                v.push(b);
                w.push(c);
            }
        }
        let (ps, t2) = (0..GLOBAL_PLANE)
            .map(|index| {
                let (lon, lat) = coords(index);
                surface(lon, lat)
            })
            .unzip();
        Self {
            valid_time,
            u,
            v,
            w,
            ps,
            t2,
        }
    }

    /// Rounds the fields to stored counts.
    pub fn quantize(&self) -> GlobalCounts {
        let n = LEVELS_PA.len() * GLOBAL_PLANE;
        assert!(self.u.len() == n && self.v.len() == n && self.w.len() == n);
        assert!(self.ps.len() == GLOBAL_PLANE && self.t2.len() == GLOBAL_PLANE);
        GlobalCounts {
            valid_time: self.valid_time,
            u: self.u.iter().map(|x| quantize(*x, WIND_SCALE)).collect(),
            v: self.v.iter().map(|x| quantize(*x, WIND_SCALE)).collect(),
            w: self.w.iter().map(|x| quantize(*x, OMEGA_SCALE)).collect(),
            ps: self.ps.iter().map(|x| quantize_ps(*x)).collect(),
            t2: self.t2.iter().map(|x| quantize_t(*x)).collect(),
        }
    }
}

impl GlobalCounts {
    /// Cuts out one tile of one altitude band.
    pub fn tile(&self, tile: TileId, band: BandId) -> Tile {
        let (first, last) = BANDS[band as usize];
        let levels = last - first + 1;
        let cut = |field: &[i16], level: usize, out: &mut Vec<i16>| {
            let base = level * GLOBAL_PLANE;
            for j in 0..TILE_N {
                // Along an edge shared with a coarser row, only every
                // `coarse`-th longitude counts; the rest are interpolated.
                let coarse = edge_stride(tile, j);
                for i in 0..TILE_N {
                    let (col, row) = global_index(tile, i, j);
                    let line = base + row * NLON;
                    let west = col / coarse * coarse;
                    let a = f32::from(field[line + west]);
                    let b = f32::from(field[line + (west + coarse) % NLON]);
                    let across = (col - west) as f32 / coarse as f32;
                    out.push((a + (b - a) * across).round() as i16);
                }
            }
        };
        let (mut u, mut v, mut w, mut ps, mut t2) = (
            Vec::with_capacity(levels * PLANE),
            Vec::with_capacity(levels * PLANE),
            Vec::with_capacity(levels * PLANE),
            Vec::with_capacity(PLANE),
            Vec::with_capacity(PLANE),
        );
        for level in first..=last {
            cut(&self.u, level, &mut u);
            cut(&self.v, level, &mut v);
            cut(&self.w, level, &mut w);
        }
        cut(&self.ps, 0, &mut ps);
        cut(&self.t2, 0, &mut t2);
        Tile {
            valid_time: self.valid_time,
            tile,
            band,
            levels_pa: LEVELS_PA[first..=last].to_vec(),
            wind_scale: WIND_SCALE,
            omega_scale: OMEGA_SCALE,
            u,
            v,
            w,
            ps,
            t2,
        }
    }

    /// Cuts out every tile of every band.
    pub fn tiles(&self) -> impl Iterator<Item = Tile> + '_ {
        (0..TILE_COUNT).flat_map(move |tile| {
            (0..BAND_COUNT).map(move |band| self.tile(tile as TileId, band as BandId))
        })
    }
}
