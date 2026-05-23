//! ACP (Agent Client Protocol) Client Framework
//!
//! A Rust implementation of the ACP protocol for connecting to coding agents.

pub mod agent;
pub mod capabilities;
pub mod codec;
pub mod error;
pub mod protocol;
pub mod registry;
pub mod session;
pub mod transport;

pub mod coding_agent;

pub use agent::*;
pub use capabilities::*;
pub use codec::*;
pub use error::*;
pub use protocol::*;
pub use registry::*;
pub use session::*;
pub use transport::*;

pub use coding_agent::ClaudeCodeAgent;
