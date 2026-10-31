//! The running app: friends, presence and calls, on top of [`Node`] and [`Call`].
//!
//! The service keeps one connection open to every friend who is online. Each
//! connection carries a control stream (relationship, names, call signaling)
//! and, during a call, voice datagrams. The UI drives it through the methods
//! here and redraws from [`Service::state`] whenever an [`Event`] arrives.
//!
//! A call is a mesh: everyone in it talks to everyone else directly. Anyone in a
//! call can ring another friend in; once they answer, they join every other member
//! themselves ([`Control::Join`]) and learn about anyone they missed from the replies
//! ([`Control::Members`]). Members who aren't friends with each other connect anyway
//! for the length of the call ("guests"), and that connection goes away with it.

pub mod control;

use std::{
    collections::{HashMap, HashSet},
    hash::{BuildHasher, RandomState},
    path::Path,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, bail};
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
use control::{CallId, Control, Member, Relation};

/// How often we try to reach friends who aren't connected.
const DIAL_INTERVAL: Duration = Duration::from_secs(10);
const DIAL_TIMEOUT: Duration = Duration::from_secs(15);
/// An unanswered ring gives up after this long.
const RING_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a newcomer keeps trying to reach a call member it has no connection to.
const JOIN_TIMEOUT: Duration = Duration::from_secs(20);
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersonView {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallView {
    pub phase: CallPhase,
    /// Who is ringing us (incoming calls only).
    pub caller: Option<PersonView>,
    /// The people we're talking with, in the order they joined. For an incoming call:
    /// the people already in the call besides the caller.
    pub members: Vec<PersonView>,
    /// People we rang into the call who haven't answered yet.
    pub ringing: Vec<PersonView>,
    /// Unix milliseconds when the first person joined (active calls only).
    pub since_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CallPhase {
    /// We're ringing people and nobody has answered yet.
    Outgoing,
    /// Someone is ringing us.
    Incoming,
    Active,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallStatsView {
    /// The worst path in the call: "relayed" if anyone's voice goes through a relay.
    pub route: String,
    /// The slowest member's round trip.
    pub ping_ms: Option<u64>,
    pub buffer_ms: u64,
    pub lost: u64,
    pub recovered: u64,
    pub mic_level: f32,
    /// Everyone's voices together.
    pub speaker_level: f32,
    pub transmitting: bool,
    pub peers: Vec<PeerStatsView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerStatsView {
    pub id: String,
    pub route: String,
    pub ping_ms: Option<u64>,
    pub buffer_ms: u64,
    pub lost: u64,
    pub recovered: u64,
    /// Their voice as we last played it.
    pub level: f32,
}

/// Someone ringing us into their call.
struct Ring {
    call: CallId,
    from: EndpointId,
    /// Already talking in that call, besides `from`, with the names the caller knows them by.
    members: Vec<Member>,
}

/// The call we're in, from our side.
struct GroupCall {
    id: CallId,
    /// Mic and speakers, opened when the first person joins.
    audio: Option<Call>,
    /// The people we're talking with, in the order they joined.
    members: Vec<EndpointId>,
    /// People we rang who haven't answered, and since when.
    ringing: HashMap<EndpointId, Instant>,
    /// Members we still have to connect to before we can join them.
    connecting: HashSet<EndpointId>,
    /// When the first person joined.
    since: Option<SystemTime>,
}

impl GroupCall {
    fn new(id: CallId) -> Self {
        Self {
            id,
            audio: None,
            members: Vec::new(),
            ringing: HashMap::new(),
            connecting: HashSet::new(),
            since: None,
        }
    }

    /// Nobody in it and nobody on the way: the call is over.
    fn is_empty(&self) -> bool {
        self.members.is_empty() && self.ringing.is_empty() && self.connecting.is_empty()
    }

    fn involves(&self, id: &EndpointId) -> bool {
        self.members.contains(id) || self.ringing.contains_key(id) || self.connecting.contains(id)
    }

    /// Takes `id` out of the call. Returns whether they were talking in it.
    fn remove(&mut self, id: &EndpointId) -> bool {
        self.ringing.remove(id);
        self.connecting.remove(id);
        let was_member = self.members.contains(id);
        self.members.retain(|m| m != id);
        if let Some(audio) = &self.audio {
            audio.remove_peer(id);
        }
        was_member
    }

    fn others(&self, except: &EndpointId) -> Vec<EndpointId> {
        self.members.iter().filter(|m| *m != except).copied().collect()
    }
}

enum CallSlot {
    Idle,
    Ringing(Ring),
    InCall(GroupCall),
}

impl CallSlot {
    fn call_id(&self) -> Option<CallId> {
        match self {
            CallSlot::Idle => None,
            CallSlot::Ringing(r) => Some(r.call),
            CallSlot::InCall(g) => Some(g.id),
        }
    }

    fn involves(&self, id: &EndpointId) -> bool {
        match self {
            CallSlot::Idle => false,
            CallSlot::Ringing(r) => r.from == *id || r.members.iter().any(|m| m.id == *id),
            CallSlot::InCall(g) => g.involves(id),
        }
    }

    /// The call with this ID, if we're in it.
    fn group(&mut self, call: CallId) -> Option<&mut GroupCall> {
        match self {
            CallSlot::InCall(g) if g.id == call => Some(g),
            _ => None,
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
    /// Call members who aren't our friends, with the names they gave. Only kept during a call.
    guests: Mutex<HashMap<EndpointId, String>>,
    // Lock order: `stored`, then `peers`, then `call`, then `guests`. Never send while
    // holding `call`: sending takes `peers`.
    call: Mutex<CallSlot>,
    controls: Arc<Controls>,
    ptt: ptt::PushToTalk,
    online: AtomicBool,
    /// Off in tests: calls track who's in them but never touch the sound card.
    open_audio: bool,
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
        Self::start_with(data_dir, true, on_event).await
    }

    /// Like [`Service::start`], but calls never open the mic or speakers. For tests.
    pub async fn start_without_audio(
        data_dir: &Path,
        on_event: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Self> {
        Self::start_with(data_dir, false, on_event).await
    }

    async fn start_with(
        data_dir: &Path,
        open_audio: bool,
        on_event: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Self> {
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
            guests: Mutex::default(),
            call: Mutex::new(CallSlot::Idle),
            controls,
            ptt,
            online: AtomicBool::new(false),
            open_audio,
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
        self.hang_up();
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
            let guests = self.0.guests.lock().unwrap();
            let person = |id: &EndpointId| PersonView {
                id: id.to_string(),
                name: stored
                    .contacts
                    .get(id)
                    .map(|c| c.name.clone())
                    .or_else(|| guests.get(id).cloned())
                    .unwrap_or_else(|| id.fmt_short().to_string()),
            };
            match &*slot {
                CallSlot::Idle => None,
                CallSlot::Ringing(r) => Some(CallView {
                    phase: CallPhase::Incoming,
                    caller: Some(person(&r.from)),
                    // Not necessarily our friends: use the names the caller gave.
                    members: r
                        .members
                        .iter()
                        .map(|m| PersonView {
                            id: m.id.to_string(),
                            name: stored.contacts.get(&m.id).map_or_else(|| m.name.clone(), |c| c.name.clone()),
                        })
                        .collect(),
                    ringing: Vec::new(),
                    since_ms: None,
                }),
                CallSlot::InCall(g) => {
                    let mut ringing: Vec<_> = g.ringing.iter().collect();
                    ringing.sort_by_key(|(_, since)| **since);
                    Some(CallView {
                        phase: if g.members.is_empty() { CallPhase::Outgoing } else { CallPhase::Active },
                        caller: None,
                        members: g.members.iter().map(person).collect(),
                        ringing: ringing.into_iter().map(|(id, _)| person(id)).collect(),
                        since_ms: g.since.map(unix_ms),
                    })
                }
            }
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
        let CallSlot::InCall(GroupCall { audio: Some(audio), .. }) = &*slot else { return None };
        let s = audio.stats();
        drop(slot);

        let route_name = |r: Route| match r {
            Route::Direct => "direct",
            Route::Relayed => "relayed",
            Route::Unknown => "unknown",
        };
        let peers: Vec<PeerStatsView> = s
            .peers
            .iter()
            .map(|p| PeerStatsView {
                id: p.id.to_string(),
                route: route_name(p.route).into(),
                ping_ms: p.rtt.map(|r| r.as_millis() as u64),
                buffer_ms: p.jitter.target as u64 * 20,
                lost: p.jitter.lost,
                recovered: p.jitter.recovered,
                level: p.level,
            })
            .collect();
        let route = if s.peers.iter().any(|p| p.route == Route::Relayed) {
            Route::Relayed
        } else if s.peers.iter().any(|p| p.route == Route::Unknown) {
            Route::Unknown
        } else {
            Route::Direct
        };
        Some(CallStatsView {
            route: route_name(route).into(),
            ping_ms: peers.iter().filter_map(|p| p.ping_ms).max(),
            buffer_ms: peers.iter().map(|p| p.buffer_ms).max().unwrap_or(0),
            lost: peers.iter().map(|p| p.lost).sum(),
            recovered: peers.iter().map(|p| p.recovered).sum(),
            mic_level: s.mic_level,
            speaker_level: s.speaker_level,
            transmitting: s.transmitting,
            peers,
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
        self.0.guests.lock().unwrap().remove(&id);
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
        self.leave_with(id);
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
    fn reconcile(&self, id: EndpointId, name: String, theirs: Relation, their_call: Option<CallId>) {
        use ContactState::*;
        let mine = self.contact_state(&id);
        debug!(peer = %id.fmt_short(), ?mine, ?theirs, ?their_call, "reconcile");

        let (new_state, mut keep_connection) = match (mine, theirs) {
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

        // Someone in our call who isn't a friend: keep talking for the length of the call.
        if !keep_connection && new_state.is_none() && self.is_call_guest(&id, their_call) {
            self.0.guests.lock().unwrap().insert(id, name.clone());
            keep_connection = true;
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
            self.leave_with(&id);
            self.drop_peer(&id, CLOSE_NOT_FRIENDS);
        }
    }

    /// Whether `id` belongs in the call we're in, either because we know them as a
    /// member or because they say they're in it too.
    fn is_call_guest(&self, id: &EndpointId, their_call: Option<CallId>) -> bool {
        let slot = self.0.call.lock().unwrap();
        slot.involves(id) || (their_call.is_some() && their_call == slot.call_id())
    }

    // ----- calls -----

    /// Rings a friend: starts a call, or adds them to the one we're in.
    pub fn call(&self, id: &EndpointId) -> Result<()> {
        if self.contact_state(id) != Some(ContactState::Friend) {
            bail!("you can only call friends");
        }
        if !self.0.peers.lock().unwrap().contains_key(id) {
            bail!("{} is offline", self.display_name(id));
        }
        let (call, members) = {
            let mut slot = self.0.call.lock().unwrap();
            if let CallSlot::Idle = &*slot {
                *slot = CallSlot::InCall(GroupCall::new(new_call_id()));
            }
            let CallSlot::InCall(g) = &mut *slot else { bail!("answer or decline the incoming call first") };
            if g.involves(id) {
                bail!("{} is already in the call", self.display_name(id));
            }
            g.ringing.insert(*id, Instant::now());
            (g.id, g.members.clone())
        };
        let members = members.into_iter().map(|m| Member { id: m, name: self.display_name(&m) }).collect();
        // A ring to someone new also tells everyone else which call we're in.
        self.broadcast_hello();
        self.send(id, Control::Invite { call, members });
        self.changed();

        let (service, id) = (self.clone(), *id);
        self.0.rt.spawn(async move {
            tokio::time::sleep(RING_TIMEOUT).await;
            let (timed_out, over) = {
                let mut slot = service.0.call.lock().unwrap();
                match slot.group(call) {
                    Some(g) if g.ringing.get(&id).is_some_and(|t| t.elapsed() >= RING_TIMEOUT) => {
                        g.ringing.remove(&id);
                        (true, g.is_empty())
                    }
                    _ => (false, false),
                }
            };
            if timed_out {
                service.send(&id, Control::Leave { call });
                service.notice(Notice::NoAnswer { name: service.display_name(&id) });
                if over {
                    service.end_call(call);
                }
                service.changed();
            }
        });
        Ok(())
    }

    pub fn accept_call(&self) -> Result<()> {
        let me = self.0.node.id();
        let (call, from, members) = {
            let mut slot = self.0.call.lock().unwrap();
            let CallSlot::Ringing(ring) = &*slot else { bail!("nobody is calling") };
            let (call, from) = (ring.call, ring.from);
            let members: Vec<_> = ring.members.iter().filter(|m| m.id != me && m.id != from).cloned().collect();
            let mut g = GroupCall::new(call);
            g.connecting.extend(members.iter().map(|m| m.id));
            *slot = CallSlot::InCall(g);
            (call, from, members)
        };
        // Until they say hello themselves, call strangers by the names the caller gave.
        let strangers: Vec<_> = members.iter().filter(|m| self.contact_state(&m.id).is_none()).collect();
        self.0.guests.lock().unwrap().extend(strangers.into_iter().map(|m| (m.id, m.name.clone())));
        let members: Vec<_> = members.into_iter().map(|m| m.id).collect();
        self.broadcast_hello();
        self.send(&from, Control::Accept { call });
        self.admit(call, from)?;
        for member in members {
            self.reach(call, member);
        }
        Ok(())
    }

    pub fn decline_call(&self) {
        let ring = {
            let mut slot = self.0.call.lock().unwrap();
            match std::mem::replace(&mut *slot, CallSlot::Idle) {
                CallSlot::Ringing(r) => Some(r),
                other => {
                    *slot = other;
                    None
                }
            }
        };
        if let Some(r) = ring {
            self.send(&r.from, Control::Decline { call: r.call });
            self.changed();
        }
    }

    /// Leaves the call (telling everyone in it, and cancelling our rings), or declines a ring.
    pub fn hang_up(&self) {
        let left = {
            let mut slot = self.0.call.lock().unwrap();
            match std::mem::replace(&mut *slot, CallSlot::Idle) {
                CallSlot::InCall(g) => Some(g),
                CallSlot::Ringing(r) => {
                    *slot = CallSlot::Ringing(r);
                    None
                }
                CallSlot::Idle => None,
            }
        };
        let Some(g) = left else { return self.decline_call() };
        let everyone = g.members.iter().chain(g.ringing.keys()).chain(g.connecting.iter()).copied();
        for id in everyone.collect::<Vec<_>>() {
            self.send(&id, Control::Leave { call: g.id });
        }
        drop(g); // closes mic and speakers
        self.after_call();
        self.changed();
    }

    /// Adds `peer` to the call's audio, opening mic and speakers if they're the first.
    /// If that fails, we leave the call.
    fn admit(&self, call: CallId, peer: EndpointId) -> Result<()> {
        let Some(conn) = self.0.peers.lock().unwrap().get(&peer).map(|p| p.conn.clone()) else { return Ok(()) };
        let needs_audio = {
            let mut slot = self.0.call.lock().unwrap();
            let Some(g) = slot.group(call) else { return Ok(()) };
            g.ringing.remove(&peer);
            g.connecting.remove(&peer);
            if !g.members.contains(&peer) {
                g.members.push(peer);
            }
            g.since.get_or_insert_with(SystemTime::now);
            g.audio.is_none()
        };
        if needs_audio && self.0.open_audio {
            // Opening devices takes a moment; don't hold the lock for it.
            match Call::start(&self.audio_devices(), self.0.controls.clone()) {
                Ok(audio) => {
                    if let Some(g) = self.0.call.lock().unwrap().group(call) {
                        g.audio.get_or_insert(audio);
                    }
                }
                Err(e) => {
                    self.hang_up();
                    return Err(e.context("couldn't open your mic or speakers"));
                }
            }
        }
        if let Some(g) = self.0.call.lock().unwrap().group(call)
            && let Some(audio) = &g.audio
        {
            // Call::add_peer spawns its receive task on the runtime.
            let _rt = self.0.rt.enter();
            if let Err(e) = audio.add_peer(conn) {
                warn!(peer = %peer.fmt_short(), "couldn't add to the call: {e:#}");
            }
        }
        self.changed();
        Ok(())
    }

    /// Joins `member` of the call we just answered: right away if we're connected,
    /// otherwise once a connection comes up (see `attach`).
    fn reach(&self, call: CallId, member: EndpointId) {
        if member == self.0.node.id() {
            return;
        }
        let connected = self.0.peers.lock().unwrap().contains_key(&member);
        {
            let mut slot = self.0.call.lock().unwrap();
            let Some(g) = slot.group(call) else { return };
            if g.members.contains(&member) {
                return;
            }
            g.connecting.insert(member);
        }
        if connected {
            self.join(call, member);
            return;
        }
        self.0.rt.spawn(self.clone().dial(member));
        // Give up on a member we can't reach, so the call doesn't wait for them forever.
        let service = self.clone();
        self.0.rt.spawn(async move {
            tokio::time::sleep(JOIN_TIMEOUT).await;
            let (gave_up, over) = {
                let mut slot = service.0.call.lock().unwrap();
                match slot.group(call) {
                    Some(g) if g.connecting.contains(&member) => {
                        g.connecting.remove(&member);
                        (true, g.is_empty())
                    }
                    _ => (false, false),
                }
            };
            if gave_up {
                warn!(peer = %member.fmt_short(), "couldn't reach a call member");
                if over {
                    service.end_call(call);
                }
                service.changed();
            }
        });
    }

    fn join(&self, call: CallId, member: EndpointId) {
        self.send(&member, Control::Join { call });
        if let Err(e) = self.admit(call, member) {
            self.notice(Notice::Error { message: format!("{e:#}") });
        }
    }

    /// Takes `id` out of whatever call we share with them, quietly.
    fn leave_with(&self, id: &EndpointId) {
        let ended = {
            let mut slot = self.0.call.lock().unwrap();
            match &mut *slot {
                CallSlot::InCall(g) if g.involves(id) => {
                    g.remove(id);
                    g.is_empty().then_some(g.id)
                }
                CallSlot::Ringing(r) if r.from == *id => {
                    *slot = CallSlot::Idle;
                    None
                }
                _ => None,
            }
        };
        if let Some(call) = ended {
            self.end_call(call);
        }
        self.changed();
    }

    /// Ends `call` because nobody is left in it.
    fn end_call(&self, call: CallId) {
        {
            let mut slot = self.0.call.lock().unwrap();
            if slot.group(call).is_none() {
                return;
            }
            *slot = CallSlot::Idle;
        }
        self.after_call();
        self.changed();
    }

    /// Lets go of the people we only talked to because of the call.
    fn after_call(&self) {
        let guests: Vec<_> = self.0.guests.lock().unwrap().drain().map(|(id, _)| id).collect();
        for id in guests {
            if self.contact_state(&id).is_none() {
                self.drop_peer(&id, CLOSE_NOT_FRIENDS);
            }
        }
        self.broadcast_hello();
    }

    /// Re-opens audio devices mid-call, e.g. after the user picked another mic.
    fn restart_call_audio(&self) {
        let (call, members) = {
            let mut slot = self.0.call.lock().unwrap();
            let CallSlot::InCall(g) = &mut *slot else { return };
            if g.audio.is_none() {
                return;
            }
            // Drop the old streams before opening new ones.
            g.audio = None;
            (g.id, g.members.clone())
        };
        for member in members {
            if let Err(e) = self.admit(call, member) {
                self.notice(Notice::Error { message: format!("{e:#}") });
                return;
            }
        }
    }

    fn audio_devices(&self) -> AudioDevices {
        let s = &self.0.stored.lock().unwrap().settings;
        AudioDevices { mic: s.mic.clone(), speakers: s.speakers.clone() }
    }

    fn on_call_message(&self, from: EndpointId, msg: Control) {
        let me = self.0.node.id();
        // Read contacts before taking the call lock; `state()` locks them in the other order.
        let name = self.display_name(&from);
        let is_friend = self.contact_state(&from) == Some(ContactState::Friend);
        match msg {
            Control::Invite { call, members } => {
                enum Answer {
                    Ring,
                    Busy,
                    /// We were ringing each other at the same time.
                    TakeTheirs,
                    Ignore,
                }
                let answer = {
                    let mut slot = self.0.call.lock().unwrap();
                    match &*slot {
                        CallSlot::Idle if is_friend => {
                            *slot = CallSlot::Ringing(Ring { call, from, members });
                            Answer::Ring
                        }
                        // Both rang each other: the call of whoever has the smaller ID wins,
                        // which both sides work out the same way.
                        CallSlot::InCall(g)
                            if g.members.is_empty()
                                && g.connecting.is_empty()
                                && g.ringing.len() == 1
                                && g.ringing.contains_key(&from) =>
                        {
                            if from < me {
                                *slot = CallSlot::Ringing(Ring { call, from, members });
                                Answer::TakeTheirs
                            } else {
                                Answer::Ignore
                            }
                        }
                        _ => Answer::Busy,
                    }
                };
                match answer {
                    Answer::Ring => self.changed(),
                    Answer::Busy => self.send(&from, Control::Decline { call }),
                    Answer::TakeTheirs => {
                        if let Err(e) = self.accept_call() {
                            self.notice(Notice::Error { message: format!("{e:#}") });
                        }
                    }
                    Answer::Ignore => {}
                }
            }
            Control::Accept { call } => {
                let others = {
                    let mut slot = self.0.call.lock().unwrap();
                    match slot.group(call) {
                        Some(g) if g.ringing.contains_key(&from) => Some(g.others(&from)),
                        _ => None,
                    }
                };
                if let Some(others) = others {
                    if let Err(e) = self.admit(call, from) {
                        self.notice(Notice::Error { message: format!("{e:#}") });
                        return;
                    }
                    // They join the others themselves; tell them who's here now.
                    self.send(&from, Control::Members { call, members: others });
                }
            }
            Control::Decline { call } => {
                let over = {
                    let mut slot = self.0.call.lock().unwrap();
                    match slot.group(call) {
                        Some(g) if g.ringing.contains_key(&from) => {
                            g.ringing.remove(&from);
                            Some(g.is_empty())
                        }
                        _ => None,
                    }
                };
                if let Some(over) = over {
                    self.notice(Notice::CallDeclined { name });
                    if over {
                        self.end_call(call);
                    }
                    self.changed();
                }
            }
            Control::Join { call } => {
                let in_call = self.0.call.lock().unwrap().group(call).map(|g| g.others(&from));
                match in_call {
                    Some(others) => {
                        if let Err(e) = self.admit(call, from) {
                            self.notice(Notice::Error { message: format!("{e:#}") });
                            return;
                        }
                        self.send(&from, Control::Members { call, members: others });
                    }
                    // We already left: don't wait for us.
                    None => self.send(&from, Control::Leave { call }),
                }
            }
            Control::Members { call, members } => {
                let missing: Vec<_> = {
                    let mut slot = self.0.call.lock().unwrap();
                    let Some(g) = slot.group(call) else { return };
                    members.into_iter().filter(|m| *m != me && !g.involves(m)).collect()
                };
                for member in missing {
                    self.reach(call, member);
                }
            }
            Control::Leave { call } => {
                let (ended, missed) = {
                    let mut slot = self.0.call.lock().unwrap();
                    match &mut *slot {
                        CallSlot::InCall(g) if g.id == call => {
                            g.remove(&from);
                            (g.is_empty(), false)
                        }
                        CallSlot::Ringing(r) if r.call == call && r.from == from => {
                            *slot = CallSlot::Idle;
                            (false, true)
                        }
                        CallSlot::Ringing(r) if r.call == call => {
                            r.members.retain(|m| m.id != from);
                            (false, false)
                        }
                        _ => return,
                    }
                };
                if missed {
                    self.notice(Notice::MissedCall { name });
                }
                if ended {
                    self.end_call(call);
                }
                self.changed();
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

        // A call member we were waiting to reach.
        let waiting = {
            let mut slot = self.0.call.lock().unwrap();
            match &mut *slot {
                CallSlot::InCall(g) if g.connecting.contains(&peer) => Some(g.id),
                _ => None,
            }
        };
        if let Some(call) = waiting {
            self.join(call, peer);
        }
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
        if removed.is_none() {
            return;
        }
        info!(peer = %peer.fmt_short(), "disconnected");
        let (was_member, ended) = {
            let mut slot = self.0.call.lock().unwrap();
            match &mut *slot {
                CallSlot::InCall(g) if g.involves(&peer) => {
                    let was_member = g.remove(&peer);
                    (was_member, g.is_empty().then_some(g.id))
                }
                CallSlot::Ringing(r) if r.from == peer => {
                    *slot = CallSlot::Idle;
                    (false, None)
                }
                _ => (false, None),
            }
        };
        if was_member {
            self.notice(Notice::ConnectionLost { name: self.display_name(&peer) });
        }
        if let Some(call) = ended {
            self.end_call(call);
        }
        self.0.guests.lock().unwrap().remove(&peer);
        self.changed();
    }

    fn on_message(&self, from: EndpointId, msg: Control) {
        match msg {
            Control::Hello { name, relation, call } => {
                let name = clean_name(&name).unwrap_or_else(|_| from.fmt_short().to_string());
                self.reconcile(from, name, relation, call);
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
        let call = self.0.call.lock().unwrap().call_id();
        self.send(&to, Control::Hello { name, relation, call });
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
        if let Some(c) = self.0.stored.lock().unwrap().contacts.get(id) {
            return c.name.clone();
        }
        self.0.guests.lock().unwrap().get(id).cloned().unwrap_or_else(|| id.fmt_short().to_string())
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

fn new_call_id() -> CallId {
    RandomState::new().hash_one(SystemTime::now())
}

fn unix_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}
