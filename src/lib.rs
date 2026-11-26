//! NIRUSETTINGS library: the schemas, the configuration files and their
//! settings, checking and writing a change, and what the system is asked.
//! The window lives in the binary (`src/main.rs`, `src/app.rs`,
//! `src/draw.rs`, `src/gui`).
pub mod cli;
pub mod desktop;
pub mod keys;
pub mod paths;
pub mod schema;
pub mod sources;
pub mod store;
pub mod validate;
pub mod writer;
