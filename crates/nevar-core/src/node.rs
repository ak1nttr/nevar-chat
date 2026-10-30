use std::{fmt, time::Duration};

use anyhow::{Context, Result};
use iroh::{
    Endpoint, EndpointId, SecretKey,
    endpoint::{Connection, presets},
};

/// ALPN for all nevar traffic. Bump the version on breaking protocol changes.
pub const ALPN: &[u8] = b"nevar/0";

/// A running peer: one iroh endpoint that both dials and accepts.
#[derive(Clone)]
pub struct Node {
    endpoint: Endpoint,
}

impl Node {
    /// Binds the endpoint and waits until it is reachable through a relay,
    /// which also publishes our address so friends can dial us by ID alone.
    pub async fn start(secret_key: SecretKey) -> Result<Self> {
        let node = Self::bind(secret_key).await?;
        node.wait_online().await;
        Ok(node)
    }

    /// Binds the endpoint without waiting to get online.
    pub async fn bind(secret_key: SecretKey) -> Result<Self> {
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .context("binding endpoint")?;
        Ok(Self { endpoint })
    }

    /// Resolves once we're reachable through a relay (and so dialable by ID).
    pub async fn wait_online(&self) {
        self.endpoint.online().await;
    }

    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    pub async fn connect(&self, peer: EndpointId) -> Result<Connection> {
        self.endpoint
            .connect(peer, ALPN)
            .await
            .with_context(|| format!("connecting to {}", peer.fmt_short()))
    }

    /// Waits for the next incoming connection. Returns `None` once the node is shut down.
    pub async fn accept(&self) -> Option<Result<Connection>> {
        let incoming = self.endpoint.accept().await?;
        Some(async { Ok(incoming.accept()?.await?) }.await)
    }

    pub async fn shutdown(&self) {
        self.endpoint.close().await;
    }
}

/// Which way packets to a peer are currently flowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Hole punching worked: packets go straight between the two PCs.
    Direct,
    /// Packets are bounced through a public relay server.
    Relayed,
    Unknown,
}

impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Route::Direct => "direct",
            Route::Relayed => "relayed",
            Route::Unknown => "unknown",
        })
    }
}

/// Reports the route the connection is currently using.
pub fn current_route(conn: &Connection) -> Route {
    current_path(conn).0
}

/// Reports the current route and its round-trip time estimate.
pub fn current_path(conn: &Connection) -> (Route, Option<Duration>) {
    let paths = conn.paths();
    match paths.iter().find(|p| p.is_selected()) {
        Some(p) if p.is_ip() => (Route::Direct, Some(p.rtt())),
        Some(p) if p.is_relay() => (Route::Relayed, Some(p.rtt())),
        _ => (Route::Unknown, None),
    }
}
