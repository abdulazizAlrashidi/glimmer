//! # Glimmer
//!
//! An original neon maze-chase arcade game. The graphics, the pixel font, the
//! window icon and all the sound effects and music are generated procedurally
//! at runtime, so the game needs no asset files and never touches the network.
//!
//! Module map:
//! - pure simulation (no window needed, fully unit-testable):
//!   [`geom`], [`config`], [`rng`], [`maze`], [`mazes`], [`actor`], [`player`],
//!   [`shade`], [`game`], [`autopilot`], [`synth`], [`save`]
//! - presentation (needs a macroquad window/audio context):
//!   [`view`], [`font`], [`icon`], [`render`], [`audio`], [`input`], [`app`]

pub mod actor;
pub mod app;
pub mod audio;
pub mod autopilot;
pub mod config;
pub mod font;
pub mod game;
pub mod geom;
pub mod icon;
pub mod input;
pub mod maze;
pub mod mazes;
pub mod player;
pub mod render;
pub mod rng;
pub mod save;
pub mod shade;
pub mod synth;
pub mod view;
