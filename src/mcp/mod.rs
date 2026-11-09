//! MCP servers, read from each agent's own config file. Prism keeps no MCP store of its own:
//! a store would be one more plaintext copy of every token.

pub mod agents;
pub mod formats;
pub mod model;
pub mod scan;
