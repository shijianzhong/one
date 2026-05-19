//! ACP (Agent Client Protocol) Client Framework
//!
//! A Rust implementation of the ACP protocol for connecting to coding agents.

pub mod protocol;
pub mod codec;
pub mod transport;
pub mod session;
pub mod capabilities;
pub mod error;
pub mod agent;
pub mod registry;

pub mod coding_agent;

pub use protocol::*;
pub use codec::*;
pub use transport::*;
pub use session::*;
pub use capabilities::*;
pub use error::*;
pub use agent::*;
pub use registry::*;

pub use coding_agent::ClaudeCodeAgent;
