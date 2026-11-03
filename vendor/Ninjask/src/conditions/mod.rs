//! Conditions: statuses, volatiles, weather, terrain and side conditions.
//!
//! Durations and residual orders come from the exported condition data; item
//! extensions (rocks, Light Clay, Terrain Extender) are code, as in Showdown's
//! `durationCallback`s. An effect set with duration N loses a turn at every
//! residual phase, starting with the turn it was set, and ends when the count
//! reaches 0.

mod pseudo_weather;
mod side;
mod slot;
mod status;
mod terrain;
mod volatiles;
mod weather;
