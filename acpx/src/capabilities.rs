//! ACP Capability Definitions
//!
//! Capability definitions for client and agent capability negotiation.

use crate::protocol::{
    AgentCapabilities, ClientCapabilities, FsCapabilities, McpCapabilities, PromptCapabilities,
    TerminalCapabilities,
};

/// Default client capabilities - minimal set
impl Default for ClientCapabilities {
    fn default() -> Self {
        Self {
            fs: Some(FsCapabilities {
                read_text_file: true,
                write_text_file: true,
            }),
            terminal: Some(TerminalCapabilities {
                create: true,
                output: true,
                release: true,
                wait_for_exit: true,
                kill: true,
            }),
        }
    }
}

impl Default for FsCapabilities {
    fn default() -> Self {
        Self {
            read_text_file: true,
            write_text_file: true,
        }
    }
}

impl Default for TerminalCapabilities {
    fn default() -> Self {
        Self {
            create: true,
            output: true,
            release: true,
            wait_for_exit: true,
            kill: true,
        }
    }
}

/// Default agent capabilities
impl Default for AgentCapabilities {
    fn default() -> Self {
        Self {
            load_session: true,
            prompt_capabilities: PromptCapabilities::default(),
            mcp_capabilities: McpCapabilities::default(),
        }
    }
}

impl Default for PromptCapabilities {
    fn default() -> Self {
        Self {
            image: false,
            audio: false,
            embedded_context: true,
        }
    }
}

impl Default for McpCapabilities {
    fn default() -> Self {
        Self {
            http: false,
            sse: false,
        }
    }
}

/// Builder for client capabilities
#[derive(Debug, Clone)]
pub struct ClientCapabilitiesBuilder {
    capabilities: ClientCapabilities,
}

impl ClientCapabilitiesBuilder {
    pub fn new() -> Self {
        Self {
            capabilities: ClientCapabilities::default(),
        }
    }

    pub fn with_fs(mut self, read: bool, write: bool) -> Self {
        self.capabilities.fs = Some(FsCapabilities {
            read_text_file: read,
            write_text_file: write,
        });
        self
    }

    pub fn with_terminal(mut self) -> Self {
        self.capabilities.terminal = Some(TerminalCapabilities::default());
        self
    }

    pub fn without_fs(mut self) -> Self {
        self.capabilities.fs = None;
        self
    }

    pub fn without_terminal(mut self) -> Self {
        self.capabilities.terminal = None;
        self
    }

    pub fn build(self) -> ClientCapabilities {
        self.capabilities
    }
}

impl Default for ClientCapabilitiesBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for agent capabilities
#[derive(Debug, Clone)]
pub struct AgentCapabilitiesBuilder {
    capabilities: AgentCapabilities,
}

impl AgentCapabilitiesBuilder {
    pub fn new() -> Self {
        Self {
            capabilities: AgentCapabilities::default(),
        }
    }

    pub fn with_load_session(mut self, enabled: bool) -> Self {
        self.capabilities.load_session = enabled;
        self
    }

    pub fn with_image_support(mut self) -> Self {
        self.capabilities.prompt_capabilities.image = true;
        self
    }

    pub fn with_audio_support(mut self) -> Self {
        self.capabilities.prompt_capabilities.audio = true;
        self
    }

    pub fn with_embedded_context(mut self) -> Self {
        self.capabilities.prompt_capabilities.embedded_context = true;
        self
    }

    pub fn with_mcp_http(mut self) -> Self {
        self.capabilities.mcp_capabilities.http = true;
        self
    }

    pub fn with_mcp_sse(mut self) -> Self {
        self.capabilities.mcp_capabilities.sse = true;
        self
    }

    pub fn build(self) -> AgentCapabilities {
        self.capabilities
    }
}

impl Default for AgentCapabilitiesBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if client supports filesystem operations
pub fn client_supports_fs(caps: &ClientCapabilities) -> bool {
    caps.fs
        .as_ref()
        .map(|fs| fs.read_text_file || fs.write_text_file)
        .unwrap_or(false)
}

/// Check if client supports terminal operations
pub fn client_supports_terminal(caps: &ClientCapabilities) -> bool {
    caps.terminal.is_some()
}

/// Check if agent supports session loading
pub fn agent_supports_load_session(caps: &AgentCapabilities) -> bool {
    caps.load_session
}

/// Check if agent supports image content
pub fn agent_supports_image(caps: &AgentCapabilities) -> bool {
    caps.prompt_capabilities.image
}

/// Check if agent supports audio content
pub fn agent_supports_audio(caps: &AgentCapabilities) -> bool {
    caps.prompt_capabilities.audio
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_client_capabilities() {
        let caps = ClientCapabilities::default();
        assert!(caps.fs.is_some());
        assert!(caps.terminal.is_some());
    }

    #[test]
    fn test_client_capabilities_builder() {
        let caps = ClientCapabilitiesBuilder::new().without_fs().build();
        assert!(caps.fs.is_none());
    }

    #[test]
    fn test_agent_capabilities_builder() {
        let caps = AgentCapabilitiesBuilder::new()
            .with_load_session(false)
            .with_image_support()
            .build();
        assert!(!caps.load_session);
        assert!(caps.prompt_capabilities.image);
    }
}
