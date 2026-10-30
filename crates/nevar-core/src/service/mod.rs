//! The running app: friends, presence and calls, on top of [`Node`] and [`Call`].
//!
//! The service keeps one connection open to every friend who is online. Each
//! connection carries a control stream (relationship, names, call signaling)
//! and, during a call, voice datagrams. The UI drives it through the methods
//! here and redraws from [`Service::state`] whenever an [`Event`] arrives.

pub mod control;

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use iroh::endpoint::{Connection, RecvStream, SendStream, Side};
use serde::Serialize;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, info, warn};

use crate::{
    EndpointId, Node,
    audio::device::{self, DeviceList},
    identity,
    node::Route,
    ptt,
    store::{Contact, ContactState, Settings, Store, Stored},
    voice::{AudioDevices, Call, Controls},
};
use control::{Control, Relation};

/// How often we try to reach friends who aren't connected.
const DIAL_INTERVAL: Duration = Duration::from_secs(10);
const DIAL_TIMEOUT: Duration = Duration::from_secs(15);
/// An unanswered ring gives up after this long.
const RING_TIMEOUT: Duration = Duration::from_secs(30);
/// QUIC close code for a connection dropped because a better duplicate exists.
const CLOSE_DUPLICATE: u32 = 1;
const CLOSE_NOT_FRIENDS: u32 = 2;
const MAX_NAME_LEN: usize = 32;

/// Something the UI should react to.
#[derive(Debug, Clone)]
pub enum Event {
    /// Anything in [`AppState`] changed; re-read it.
    StateChanged,
    /// Something to tell the user about once.
    Notice(Notice),
}

/// One-off messages for the user; the UI words them.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Notice {
    FriendRequest { name: String },
    FriendAccepted { name: String },
    /// They turned down our call, or were busy.
    CallDeclined { name: String },
    NoAnswer { name: String },
    MissedCall { name: String },
    ConnectionLost { name: String },
    Error { message: String },
}

/// Everything the UI shows, as one snapshot.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub me: Me,
    pub contacts: Vec<ContactView>,
    pub call: Option<CallView>,
    pub settings: Settings,
    pub muted: bool,
    pub deafened: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    /// My friend code.
    pub id: String,
    pub name: String,
    /// Reachable through the relays, so friends can find us.
    pub online: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactView {
    pub id: String,
    pub name: String,
    pub state: ContactState,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallView {
    pub peer: String,
    pub name: String,
    pub phase: CallPhase,
    /// Unix milliseconds when the call connected (active calls only).
    pub since_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CallPhase {
    /// We're ringing them.
    Outgoing,
    /// They're ringing us.
    Incoming,
    Active,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallStatsView {
    pub route: String,
    pub ping_ms: Option<u64>,
    pub buffer_ms: u64,
    pub lost: u64,
    pub recovered: u64,
    pub mic_level: f32,
    pub speaker_level: f32,
    pub transmitting: bool,
}

enum CallSlot {
    Idle,
    Outgoing { peer: EndpointId, since: Instant },
    Incoming { peer: EndpointId },
    Active { peer: EndpointId, call: Call, since: SystemTime },
}

impl CallSlot {
    fn peer(&self) -> Option<EndpointId> {
        match self {
            CallSlot::Idle => None,
            CallSlot::Outgoing { peer, .. } | CallSlot::Incoming { peer } | CallSlot::Active { peer, .. } => {
                Some(*peer)
            }
        }
    }
}

/// A live connection to one peer.
struct Peer {
    conn: Connection,
    tx: mpsc::UnboundedSender<Control>,
    tasks: [JoinHandle<()>; 2],
}

impl Drop for Peer {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

type EventSink = Box<dyn Fn(Event) + Send + Sync>;

struct Inner {
    node: Node,
    store: Store,
    stored: Mutex<Stored>,
    peers: Mutex<HashMap<EndpointId, Peer>>,
    dialing: Mutex<HashSet<EndpointId>>,
    call: Mutex<CallSlot>,
    controls: Arc<Controls>,
    ptt: ptt::PushToTalk,
    online: AtomicBool,
    on_event: EventSink,
    /// Every background task goes through this, so methods are safe to call from any
    /// thread, including a UI thread outside the async runtime.
    rt: tokio::runtime::Handle,
}

#[derive(Clone)]
pub struct Service(Arc<Inner>);

impl Service {
    /// Loads identity and state from `data_dir` and starts connecting to friends.
    pub async fn start(data_dir: &Path, on_event: impl Fn(Event) + Send + Sync + 'static) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let key = identity::load_or_create(&data_dir.join("identity.key"))?;
        let store = Store::new(data_dir);
        let mut stored = store.load()?;
        if stored.name.trim().is_empty() {
            stored.name = default_name();
        }
        let node = Node::bind(key).await?;
        let controls = Arc::new(Controls::default());
        let ptt = ptt::PushToTalk::start(controls.clone());

        let service = Self(Arc::new(Inner {
            node,
            store,
            stored: Mutex::new(stored),
            peers: Mutex::default(),
            dialing: Mutex::default(),
            call: Mutex::new(CallSlot::Idle),
            controls,
            ptt,
            online: AtomicBool::new(false),
            on_event: Box::new(on_event),
            rt: tokio::runtime::Handle::current(),
        }));
        service.apply_settings(&service.0.stored.lock().unwrap().settings.clone());

        service.0.rt.spawn(service.clone().accept_loop());
        service.0.rt.spawn(service.clone().dial_loop());
        service.0.rt.spawn({
            let s = service.clone();
            async move {
                s.0.node.wait_online().await;
                s.0.online.store(true, Ordering::Relaxed);
                info!("online as {}", s.0.node.id());
                s.changed();
            }
        });
        Ok(service)
    }

    pub async fn shutdown(&self) {
        self.end_call(None);
        self.0.peers.lock().unwrap().clear();
        self.0.node.shutdown().await;
    }

    // ----- state for the UI -----

    pub fn state(&self) -> AppState {
        let stored = self.0.stored.lock().unwrap();
        let peers = self.0.peers.lock().unwrap();
        let contacts = stored
            .contacts
            .iter()
            .filter(|(_, c)| c.state != ContactState::Declined)
            .map(|(id, c)| ContactView {
                id: id.to_string(),
                name: c.name.clone(),
                state: c.state,
                online: c.state == ContactState::Friend && peers.contains_key(id),
            })
            .collect();
        drop(peers);

        let call = {
            let slot = self.0.call.lock().unwrap();
            slot.peer().map(|peer| CallView {
                peer: peer.to_string(),
                name: stored.contacts.get(&peer).map_or_else(|| peer.fmt_short().to_string(), |c| c.name.clone()),
                phase: match &*slot {
                    CallSlot::Outgoing { .. } => CallPhase::Outgoing,
                    CallSlot::Incoming { .. } => CallPhase::Incoming,
                    _ => CallPhase::Active,
                },
                since_ms: match &*slot {
                    CallSlot::Active { since, .. } => Some(unix_ms(*since)),
                    _ => None,
                },
            })
        };

        AppState {
            me: Me {
                id: self.0.node.id().to_string(),
                name: stored.name.clone(),
                online: self.0.online.load(Ordering::Relaxed),
            },
            contacts,
            call,
            settings: stored.settings.clone(),
            muted: self.0.controls.muted(),
            deafened: self.0.controls.deafened(),
        }
    }

    pub fn call_stats(&self) -> Option<CallStatsView> {
        let slot = self.0.call.lock().unwrap();
        let CallSlot::Active { call, .. } = &*slot else { return None };
        let s = call.stats();
        Some(CallStatsView {
            route: match s.route {
                Route::Direct => "direct",
                Route::Relayed => "relayed",
                Route::Unknown => "unknown",
            }
            .into(),
            ping_ms: s.rtt.map(|r| r.as_millis() as u64),
            buffer_ms: s.jitter.target as u64 * 20,
            lost: s.jitter.lost,
            recovered: s.jitter.recovered,
            mic_level: s.mic_level,
            speaker_level: s.speaker_level,
            transmitting: s.transmitting,
        })
    }

    pub fn devices(&self) -> Result<DeviceList> {
        device::list_devices()
    }

    pub fn controls(&self) -> &Arc<Controls> {
        &self.0.controls
    }

    // ----- profile and settings -----

    pub fn set_name(&self, name: &str) -> Result<()> {
        let name = clean_name(name)?;
        self.update_stored(|s| s.name = name);
        self.broadcast_hello();
        Ok(())
    }

    pub fn set_settings(&self, settings: Settings) {
        let old = self.0.stored.lock().unwrap().settings.clone();
        self.apply_settings(&settings);
        self.update_stored(|s| s.settings = settings.clone());
        if old.mic != settings.mic || old.speakers != settings.speakers {
            self.restart_call_audio();
        }
    }

    /// Mute and deafen are per-session toggles, not saved settings.
    pub fn set_muted(&self, muted: bool) {
        self.0.controls.set_muted(muted);
        self.changed();
    }

    pub fn set_deafened(&self, deafened: bool) {
        self.0.controls.set_deafened(deafened);
        self.changed();
    }

    fn apply_settings(&self, s: &Settings) {
        let c = &self.0.controls;
        c.set_mode(s.mode.into());
        c.set_noise_suppression(s.noise_suppression);
        c.set_echo_cancellation(s.echo_cancellation);
        c.set_auto_gain(s.auto_gain);
        c.set_output_volume(s.output_volume);
        self.0.ptt.set_key(s.push_to_talk_key);
    }

    // ----- friends -----

    /// Sends a friend request to the owner of `code` (or accepts theirs, if they asked first).
    pub fn add_friend(&self, code: &str) -> Result<()> {
        let id = EndpointId::from_str(code.trim()).map_err(|_| anyhow::anyhow!("that doesn't look like a friend code"))?;
        if id == self.0.node.id() {
            bail!("that's your own code");
        }
        let current = self.contact_state(&id);
        match current {
            Some(ContactState::Friend) => bail!("you're already friends"),
            Some(ContactState::Incoming) => return self.accept_request(&id),
            _ => {}
        }
        self.update_stored(|s| {
            let name = s.contacts.get(&id).map_or_else(|| id.fmt_short().to_string(), |c| c.name.clone());
            s.contacts.insert(id, Contact { name, state: ContactState::Outgoing });
        });
        self.send_hello(id);
        self.0.rt.spawn(self.clone().dial(id));
        Ok(())
    }

    pub fn accept_request(&self, id: &EndpointId) -> Result<()> {
        if self.contact_state(id) != Some(ContactState::Incoming) {
            bail!("no friend request from them");
        }
        self.set_contact_state(*id, Some(ContactState::Friend));
        self.send_hello(*id);
        self.0.rt.spawn(self.clone().dial(*id));
        Ok(())
    }

    pub fn decline_request(&self, id: &EndpointId) -> Result<()> {
        if self.contact_state(id) != Some(ContactState::Incoming) {
            bail!("no friend request from them");
        }
        self.set_contact_state(*id, Some(ContactState::Declined));
        self.send_hello(*id);
        self.drop_peer(id, CLOSE_NOT_FRIENDS);
        Ok(())
    }

    /// Removes a friend or cancels a request we sent.
    pub fn remove_contact(&self, id: &EndpointId) {
        if self.0.call.lock().unwrap().peer() == Some(*id) {
            self.end_call(Some(*id));
        }
        self.set_contact_state(*id, None);
        self.send_hello(*id);
        self.drop_peer(id, CLOSE_NOT_FRIENDS);
    }

    fn contact_state(&self, id: &EndpointId) -> Option<ContactState> {
        self.0.stored.lock().unwrap().contacts.get(id).map(|c| c.state)
    }

    fn set_contact_state(&self, id: EndpointId, state: Option<ContactState>) {
        self.update_stored(|s| match state {
            Some(state) => {
                let name = s.contacts.get(&id).map_or_else(|| id.fmt_short().to_string(), |c| c.name.clone());
                s.contacts.insert(id, Contact { name, state });
            }
            None => {
                s.contacts.remove(&id);
            }
        });
    }

    /// Adjusts my view of the relationship to what they just told me.
    fn reconcile(&self, id: EndpointId, name: String, theirs: Relation) {
        use ContactState::*;
        let mine = self.contact_state(&id);
        debug!(peer = %id.fmt_short(), ?mine, ?theirs, "reconcile");

        let (new_state, keep_connection) = match (mine, theirs) {
            (Some(Friend), Relation::Friend | Relation::Requesting) => (Some(Friend), true),
            (Some(Outgoing), Relation::Friend | Relation::Requesting) => (Some(Friend), true),
            (Some(Outgoing), Relation::None) => (Some(Outgoing), true),
            (Some(Incoming) | None, Relation::Requesting) => (Some(Incoming), true),
            (Some(Declined), _) => (Some(Declined), false),
            // They removed or declined me, or withdrew their request.
            (Some(Friend | Outgoing | Incoming), _) => (None, false),
            (None, _) => (None, false),
        };

        if let Some(state) = new_state {
            let name = name.clone();
            self.update_stored(|s| {
                s.contacts.insert(id, Contact { name, state });
            });
        } else if mine.is_some() {
            self.set_contact_state(id, None);
        }

        if new_state.map(ContactState::relation) != mine.map(ContactState::relation) {
            self.send_hello(id);
        }
        if mine == Some(Outgoing) && new_state == Some(Friend) {
            self.notice(Notice::FriendAccepted { name: name.clone() });
        }
        if new_state == Some(Incoming) && mine != Some(Incoming) {
            self.notice(Notice::FriendRequest { name: self.display_name(&id) });
        }
        if !keep_connection {
            if self.0.call.lock().unwrap().peer() == Some(id) {
                self.end_call(Some(id));
            }
            self.drop_peer(&id, CLOSE_NOT_FRIENDS);
        }
    }

    // ----- calls -----

    pub fn call(&self, id: &EndpointId) -> Result<()> {
        if self.contact_state(id) != Some(ContactState::Friend) {
            bail!("you can only call friends");
        }
        if !self.0.peers.lock().unwrap().contains_key(id) {
            bail!("{} is offline", self.display_name(id));
        }
        {
            let mut slot = self.0.call.lock().unwrap();
            if !matches!(*slot, CallSlot::Idle) {
                bail!("you're already in a call");
            }
            *slot = CallSlot::Outgoing { peer: *id, since: Instant::now() };
        }
        self.send(id, Control::CallInvite);
        self.changed();

        let (service, id) = (self.clone(), *id);
        self.0.rt.spawn(async move {
            tokio::time::sleep(RING_TIMEOUT).await;
            let timed_out = matches!(
                &*service.0.call.lock().unwrap(),
                CallSlot::Outgoing { peer, since } if *peer == id && since.elapsed() >= RING_TIMEOUT
            );
            if timed_out {
                service.notice(Notice::NoAnswer { name: service.display_name(&id) });
                service.end_call(Some(id));
            }
        });
        Ok(())
    }

    pub fn accept_call(&self) -> Result<()> {
        let peer = match &*self.0.call.lock().unwrap() {
            CallSlot::Incoming { peer } => *peer,
            _ => bail!("nobody is calling"),
        };
        self.send(&peer, Control::CallAccept);
        self.start_audio(peer)
    }

    pub fn decline_call(&self) {
        let peer = match &*self.0.call.lock().unwrap() {
            CallSlot::Incoming { peer } => *peer,
            _ => return,
        };
        self.send(&peer, Control::CallDecline);
        *self.0.call.lock().unwrap() = CallSlot::Idle;
        self.changed();
    }

    pub fn hang_up(&self) {
        self.end_call(None);
    }

    /// Ends whatever call is going on (only if it's with `only_with`, when given),
    /// telling the other side.
    fn end_call(&self, only_with: Option<EndpointId>) {
        let ended = {
            let mut slot = self.0.call.lock().unwrap();
            match slot.peer() {
                Some(peer) if only_with.is_none_or(|p| p == peer) => {
                    *slot = CallSlot::Idle;
                    Some(peer)
                }
                _ => None,
            }
        };
        if let Some(peer) = ended {
            self.send(&peer, Control::CallEnd);
            self.changed();
        }
    }

    /// Opens mic and speakers and starts streaming with `peer`.
    fn start_audio(&self, peer: EndpointId) -> Result<()> {
        let conn = self
            .0
            .peers
            .lock()
            .unwrap()
            .get(&peer)
            .map(|p| p.conn.clone())
            .context("connection lost")?;
        let devices = self.audio_devices();
        // Call::start spawns its receive task on the runtime.
        let _rt = self.0.rt.enter();
        let result = Call::start(conn, &devices, self.0.controls.clone());
        let mut slot = self.0.call.lock().unwrap();
        match result {
            Ok(call) => {
                *slot = CallSlot::Active { peer, call, since: SystemTime::now() };
                drop(slot);
                self.changed();
                Ok(())
            }
            Err(e) => {
                *slot = CallSlot::Idle;
                drop(slot);
                self.send(&peer, Control::CallEnd);
                self.changed();
                Err(e.context("couldn't open your mic or speakers"))
            }
        }
    }

    /// Re-opens audio devices mid-call, e.g. after the user picked another mic.
    fn restart_call_audio(&self) {
        let peer = {
            let mut slot = self.0.call.lock().unwrap();
            let CallSlot::Active { peer, .. } = &*slot else { return };
            let peer = *peer;
            // Drop the old streams before opening new ones.
            *slot = CallSlot::Outgoing { peer, since: Instant::now() };
            peer
        };
        if let Err(e) = self.start_audio(peer) {
            self.notice(Notice::Error { message: format!("{e:#}") });
        }
    }

    fn audio_devices(&self) -> AudioDevices {
        let s = &self.0.stored.lock().unwrap().settings;
        AudioDevices { mic: s.mic.clone(), speakers: s.speakers.clone() }
    }

    fn on_call_message(&self, from: EndpointId, msg: Control) {
        // Read contacts before taking the call lock; `state()` locks them in the other order.
        let name = self.display_name(&from);
        let is_friend = self.contact_state(&from) == Some(ContactState::Friend);
        let mut slot = self.0.call.lock().unwrap();
        match msg {
            Control::CallInvite => match &*slot {
                CallSlot::Idle if is_friend => {
                    *slot = CallSlot::Incoming { peer: from };
                    drop(slot);
                    self.changed();
                }
                // We were ringing each other at the same time: just connect.
                CallSlot::Outgoing { peer, .. } if *peer == from => {
                    drop(slot);
                    self.send(&from, Control::CallAccept);
                    if let Err(e) = self.start_audio(from) {
                        self.notice(Notice::Error { message: format!("{e:#}") });
                    }
                }
                _ => {
                    drop(slot);
                    self.send(&from, Control::CallDecline);
                }
            },
            Control::CallAccept => {
                if matches!(&*slot, CallSlot::Outgoing { peer, .. } if *peer == from) {
                    drop(slot);
                    if let Err(e) = self.start_audio(from) {
                        self.notice(Notice::Error { message: format!("{e:#}") });
                    }
                }
            }
            Control::CallDecline => {
                if matches!(&*slot, CallSlot::Outgoing { peer, .. } if *peer == from) {
                    *slot = CallSlot::Idle;
                    drop(slot);
                    self.notice(Notice::CallDeclined { name });
                    self.changed();
                }
            }
            Control::CallEnd => {
                if slot.peer() == Some(from) {
                    let was_ringing_us = matches!(&*slot, CallSlot::Incoming { .. });
                    *slot = CallSlot::Idle;
                    drop(slot);
                    if was_ringing_us {
                        self.notice(Notice::MissedCall { name });
                    }
                    self.changed();
                }
            }
            Control::Hello { .. } => unreachable!("handled by on_message"),
        }
    }

    // ----- connections -----

    async fn accept_loop(self) {
        while let Some(incoming) = self.0.node.accept().await {
            let service = self.clone();
            self.0.rt.spawn(async move {
                let conn = match incoming {
                    Ok(conn) => conn,
                    Err(e) => return debug!("incoming connection failed: {e:#}"),
                };
                match conn.accept_bi().await {
                    Ok((send, recv)) => service.attach(conn, send, recv),
                    Err(e) => debug!("no control stream: {e}"),
                }
            });
        }
    }

    async fn dial_loop(self) {
        let mut ticker = tokio::time::interval(DIAL_INTERVAL);
        loop {
            ticker.tick().await;
            let wanted: Vec<EndpointId> = {
                let stored = self.0.stored.lock().unwrap();
                let peers = self.0.peers.lock().unwrap();
                stored
                    .contacts
                    .iter()
                    .filter(|(id, c)| {
                        matches!(c.state, ContactState::Friend | ContactState::Outgoing) && !peers.contains_key(*id)
                    })
                    .map(|(id, _)| *id)
                    .collect()
            };
            for id in wanted {
                self.0.rt.spawn(self.clone().dial(id));
            }
        }
    }

    async fn dial(self, id: EndpointId) {
        if !self.0.dialing.lock().unwrap().insert(id) || self.0.peers.lock().unwrap().contains_key(&id) {
            return;
        }
        let result = tokio::time::timeout(DIAL_TIMEOUT, async {
            let conn = self.0.node.connect(id).await?;
            let (send, recv) = conn.open_bi().await?;
            anyhow::Ok((conn, send, recv))
        })
        .await;
        self.0.dialing.lock().unwrap().remove(&id);
        match result {
            Ok(Ok((conn, send, recv))) => self.attach(conn, send, recv),
            Ok(Err(e)) => debug!(peer = %id.fmt_short(), "dial failed: {e:#}"),
            Err(_) => debug!(peer = %id.fmt_short(), "dial timed out"),
        }
    }

    /// Takes over a fresh connection: says hello and starts reading and writing control messages.
    fn attach(&self, conn: Connection, send: SendStream, recv: RecvStream) {
        let me = self.0.node.id();
        let peer = conn.remote_id();
        let (tx, rx) = mpsc::unbounded_channel();

        // Both sides may dial at once. Keep the connection dialed by the smaller ID;
        // both ends compute the same answer.
        let dialer = |c: &Connection| if c.side() == Side::Client { me } else { peer };
        let preferred = me.min(peer);
        {
            let mut peers = self.0.peers.lock().unwrap();
            if let Some(existing) = peers.get(&peer) {
                if dialer(&existing.conn) == preferred || dialer(&conn) != preferred {
                    conn.close(CLOSE_DUPLICATE.into(), b"duplicate");
                    return;
                }
                existing.conn.close(CLOSE_DUPLICATE.into(), b"duplicate");
            }
            let writer = self.0.rt.spawn(write_loop(send, rx));
            let reader = self.0.rt.spawn(self.clone().read_loop(peer, conn.stable_id(), recv));
            peers.insert(peer, Peer { conn: conn.clone(), tx, tasks: [writer, reader] });
        }
        info!(peer = %peer.fmt_short(), "connected");
        self.send_hello(peer);
        self.changed();
    }

    async fn read_loop(self, peer: EndpointId, conn_id: usize, mut recv: RecvStream) {
        loop {
            match control::read(&mut recv).await {
                Ok(msg) => self.on_message(peer, msg),
                Err(e) => {
                    debug!(peer = %peer.fmt_short(), "control stream ended: {e:#}");
                    break;
                }
            }
        }
        self.detach(peer, conn_id);
    }

    fn detach(&self, peer: EndpointId, conn_id: usize) {
        let removed = {
            let mut peers = self.0.peers.lock().unwrap();
            // A duplicate that lost the tie-break must not remove the winner.
            match peers.get(&peer) {
                Some(p) if p.conn.stable_id() == conn_id => peers.remove(&peer),
                _ => None,
            }
        };
        if removed.is_some() {
            info!(peer = %peer.fmt_short(), "disconnected");
            let in_call = self.0.call.lock().unwrap().peer() == Some(peer);
            if in_call {
                *self.0.call.lock().unwrap() = CallSlot::Idle;
                self.notice(Notice::ConnectionLost { name: self.display_name(&peer) });
            }
            self.changed();
        }
    }

    fn on_message(&self, from: EndpointId, msg: Control) {
        match msg {
            Control::Hello { name, relation } => {
                let name = clean_name(&name).unwrap_or_else(|_| from.fmt_short().to_string());
                self.reconcile(from, name, relation);
                self.changed();
            }
            other => self.on_call_message(from, other),
        }
    }

    fn send(&self, to: &EndpointId, msg: Control) {
        if let Some(peer) = self.0.peers.lock().unwrap().get(to) {
            let _ = peer.tx.send(msg);
        }
    }

    fn send_hello(&self, to: EndpointId) {
        let (name, relation) = {
            let stored = self.0.stored.lock().unwrap();
            let relation = stored.contacts.get(&to).map_or(Relation::None, |c| c.state.relation());
            (stored.name.clone(), relation)
        };
        self.send(&to, Control::Hello { name, relation });
    }

    fn broadcast_hello(&self) {
        let ids: Vec<_> = self.0.peers.lock().unwrap().keys().copied().collect();
        for id in ids {
            self.send_hello(id);
        }
    }

    fn drop_peer(&self, id: &EndpointId, code: u32) {
        // Let the last hello go out before closing.
        let Some(peer) = self.0.peers.lock().unwrap().remove(id) else { return };
        self.0.rt.spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            peer.conn.close(code.into(), b"");
        });
        self.changed();
    }

    // ----- helpers -----

    fn update_stored(&self, f: impl FnOnce(&mut Stored)) {
        let snapshot = {
            let mut stored = self.0.stored.lock().unwrap();
            f(&mut stored);
            stored.clone()
        };
        if let Err(e) = self.0.store.save(&snapshot) {
            warn!("saving state failed: {e:#}");
        }
        self.changed();
    }

    fn display_name(&self, id: &EndpointId) -> String {
        self.0
            .stored
            .lock()
            .unwrap()
            .contacts
            .get(id)
            .map_or_else(|| id.fmt_short().to_string(), |c| c.name.clone())
    }

    fn changed(&self) {
        (self.0.on_event)(Event::StateChanged);
    }

    fn notice(&self, notice: Notice) {
        (self.0.on_event)(Event::Notice(notice));
    }
}

async fn write_loop(mut send: SendStream, mut rx: mpsc::UnboundedReceiver<Control>) {
    while let Some(msg) = rx.recv().await {
        if let Err(e) = control::write(&mut send, &msg).await {
            debug!("control write failed: {e:#}");
            break;
        }
    }
}

fn clean_name(name: &str) -> Result<String> {
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME_LEN).collect();
    if name.is_empty() {
        bail!("name can't be empty");
    }
    Ok(name)
}

fn default_name() -> String {
    std::env::var("USERNAME").ok().and_then(|n| clean_name(&n).ok()).unwrap_or_else(|| "nevar".into())
}

fn unix_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}
