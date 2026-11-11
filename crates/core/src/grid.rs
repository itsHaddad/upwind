//! Geometry of the global wind grid and how it is cut into tiles.
//!
//! The grid is the 1° GFS grid: 360 longitudes starting at 0°E going east,
//! 181 latitudes starting at 90°N going south. It is cut into 30° tiles; each
//! tile stores 31 × 31 points so that it shares its last row and column with
//! its neighbours and any point can be interpolated from a single tile.

/// Grid spacing in degrees.
pub const CELL_DEG: f64 = 1.0;
/// Number of longitudes in the global grid.
pub const NLON: usize = 360;
/// Number of latitudes in the global grid (both poles included).
pub const NLAT: usize = 181;
/// Tile edge in grid cells.
pub const TILE_CELLS: usize = 30;
/// Points along one tile edge (cells + 1, the shared edge).
pub const TILE_N: usize = TILE_CELLS + 1;
/// Tiles around the globe.
pub const TILES_X: usize = NLON / TILE_CELLS;
/// Tiles pole to pole.
pub const TILES_Y: usize = (NLAT - 1) / TILE_CELLS;
/// Total number of tiles.
pub const TILE_COUNT: usize = TILES_X * TILES_Y;
/// Points in one horizontal plane of a tile.
pub const PLANE: usize = TILE_N * TILE_N;

/// Identifies a tile: `ty * TILES_X + tx`.
pub type TileId = u8;

/// Where a geographic point falls inside the tile grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TilePos {
    pub tile: TileId,
    /// Index of the west column of the enclosing cell, `0..TILE_CELLS`.
    pub i: usize,
    /// Index of the north row of the enclosing cell, `0..TILE_CELLS`.
    pub j: usize,
    /// Fraction of the way east across the cell, `0..=1`.
    pub ax: f32,
    /// Fraction of the way south across the cell, `0..=1`.
    pub ay: f32,
}

/// Wraps a longitude into `[0, 360)`.
pub fn wrap_lon(lon: f64) -> f64 {
    let l = lon.rem_euclid(360.0);
    if l >= 360.0 {
        0.0
    } else {
        l
    }
}

/// Locates a point (degrees east, degrees north) in the tile grid.
pub fn locate(lon: f64, lat: f64) -> TilePos {
    let gx = wrap_lon(lon) / CELL_DEG;
    let gy = (90.0 - lat.clamp(-90.0, 90.0)) / CELL_DEG;
    let (tx, i, ax) = split(gx, TILES_X);
    let (ty, j, ay) = split(gy, TILES_Y);
    TilePos {
        tile: (ty * TILES_X + tx) as TileId,
        i,
        j,
        ax,
        ay,
    }
}

/// Splits a global grid coordinate into (tile index, cell index, fraction).
fn split(g: f64, tiles: usize) -> (usize, usize, f32) {
    let t = ((g / TILE_CELLS as f64) as usize).min(tiles - 1);
    let f = g - (t * TILE_CELLS) as f64;
    let c = (f as usize).min(TILE_CELLS - 1);
    (t, c, (f - c as f64) as f32)
}

/// Global grid indices `(lon_index, lat_index)` of point `(i, j)` of a tile.
pub fn global_index(tile: TileId, i: usize, j: usize) -> (usize, usize) {
    let tx = tile as usize % TILES_X;
    let ty = tile as usize / TILES_X;
    ((tx * TILE_CELLS + i) % NLON, ty * TILE_CELLS + j)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_covers_the_whole_globe() {
        let p = locate(0.0, 90.0);
        assert_eq!((p.tile, p.i, p.j), (0, 0, 0));
        let p = locate(359.5, -90.0);
        assert_eq!(p.tile as usize, TILE_COUNT - 1);
        assert_eq!((p.i, p.j), (29, 29));
        assert!((p.ax - 0.5).abs() < 1e-6 && (p.ay - 1.0).abs() < 1e-6);
        let p = locate(-0.25, 0.0);
        assert_eq!(p.tile as usize % TILES_X, TILES_X - 1);
        assert!((p.ax - 0.75).abs() < 1e-6);
    }

    #[test]
    fn tile_edges_wrap_to_the_global_grid() {
        assert_eq!(global_index(11, 30, 0), (0, 0));
        assert_eq!(global_index(71, 0, 30), (330, 180));
    }
}
