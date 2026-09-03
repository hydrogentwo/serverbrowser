//! serverbrowser library — terminal browser on the Servo engine.

pub mod config;
pub mod mindmap;
pub mod output;
pub mod render;

// render depends on the `servo` crate; only compile it when not in a
// "no usable engine" environment. It's always available here, so it's
// unconditional.
pub mod nav;