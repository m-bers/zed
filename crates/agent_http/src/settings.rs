//! Runtime configuration read from environment variables at server startup.
//!
//! Deliberately plain env vars for v0.2 so the LAN-access path works without
//! wiring through Zed's settings store. A proper `settings::Settings`
//! integration lives in the v0.3 milestone.
//!
//! * `AGENT_HTTP_BIND` — address to bind to (default `127.0.0.1`).
//!   Set to `0.0.0.0` for LAN/phone access.
//! * `AGENT_HTTP_PORT` — TCP port (default `9292`).
//! * `AGENT_HTTP_TOKEN` — optional bearer token. When set, every request must
//!   carry `Authorization: Bearer <token>` or receive `401`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

const DEFAULT_PORT: u16 = 9292;

#[derive(Clone, Debug)]
pub struct RuntimeSettings {
    pub bind: SocketAddr,
    pub auth_token: Option<String>,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self::from_env()
    }
}

impl RuntimeSettings {
    pub fn from_env() -> Self {
        let ip = std::env::var("AGENT_HTTP_BIND")
            .ok()
            .and_then(|s| s.parse::<IpAddr>().ok())
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let port = std::env::var("AGENT_HTTP_PORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(DEFAULT_PORT);
        let auth_token = std::env::var("AGENT_HTTP_TOKEN")
            .ok()
            .filter(|s| !s.is_empty());
        Self {
            bind: SocketAddr::new(ip, port),
            auth_token,
        }
    }
}
