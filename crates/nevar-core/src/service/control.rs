//! Reliable control messages between two peers, on one QUIC stream per connection.
//! Each message is a little-endian `u32` length followed by a postcard body.

use anyhow::{Result, bail};
use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};

/// Upper bound for one message; anything larger is a broken or hostile peer.
const MAX_MESSAGE: usize = 64 * 1024;

/// How I see the other person. Sent on connect and whenever it changes,
/// so both sides can reconcile (see `Service::reconcile`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Relation {
    #[default]
    None,
    /// I sent them a friend request.
    Requesting,
    Friend,
    /// I turned down their request; don't ask again.
    Declined,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Control {
    /// First message each way, and again whenever my name or relation changes.
    Hello { name: String, relation: Relation },
    /// Ring the other side.
    CallInvite,
    CallAccept,
    /// Turned down a ring, or busy in another call.
    CallDecline,
    /// Hang up an active call or cancel a ring.
    CallEnd,
}

pub async fn write(send: &mut SendStream, msg: &Control) -> Result<()> {
    let body = postcard::to_stdvec(msg)?;
    send.write_all(&(body.len() as u32).to_le_bytes()).await?;
    send.write_all(&body).await?;
    Ok(())
}

pub async fn read(recv: &mut RecvStream) -> Result<Control> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE {
        bail!("control message too large ({len} bytes)");
    }
    let mut body = vec![0u8; len];
    recv.read_exact(&mut body).await?;
    Ok(postcard::from_bytes(&body)?)
}
