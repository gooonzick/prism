//! MCP servers, read from each agent's own config file. Prism keeps no MCP store of its own:
//! a store would be one more plaintext copy of every token.

pub mod agents;
pub mod formats;
pub mod model;
#[allow(dead_code)] // the add, copy, edit, remove, disable and merge commands use it
pub mod plan;
pub mod scan;
