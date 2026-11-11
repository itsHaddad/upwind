//! Where has the air been?
//!
//! This crate holds everything needed to follow air backwards through the
//! atmosphere: the layout of the wind grid ([`grid`]), the wind file format
//! ([`codec`]), interpolation in the loaded tiles ([`store`]) and the
//! trajectory integrator ([`tracer`]). It has no I/O and compiles to
//! WebAssembly unchanged.

pub mod build;
pub mod codec;
pub mod geo;
pub mod grid;
pub mod store;
pub mod tracer;

pub use codec::Tile;
pub use store::{Miss, Sample, TileKey, WindStore};
pub use tracer::{Start, Status, Tracer};
