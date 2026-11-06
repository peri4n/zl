//! Language server for editors, with an in-memory index of the notebook that salsa updates
//! incrementally on every edit

mod db;
mod line_index;
pub(crate) mod server;
