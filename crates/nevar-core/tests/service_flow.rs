//! Two services on one machine go through the friend and call-signaling flows.
//! Needs internet access: peers find each other through the public relays.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use nevar_core::{
    EndpointId,
    service::{AppState, CallPhase, Event, Notice, Service},
    store::ContactState,
};

struct Peer {
    service: Service,
    notices: Arc<Mutex<Vec<Notice>>>,
    dir: PathBuf,
}

impl Peer {
    async fn start(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("nevar-test-{name}-{}", rand_suffix()));
        let notices = Arc::new(Mutex::new(Vec::new()));
        let sink = notices.clone();
        // Calls here track who's in them without opening the real mic and speakers.
        let service = Service::start_without_audio(&dir, move |e| {
            if let Event::Notice(notice) = e {
                sink.lock().unwrap().push(notice);
            }
        })
        .await
        .unwrap();
        service.set_name(name).unwrap();
        Self { service, notices, dir }
    }

    fn id(&self) -> EndpointId {
        self.service.state().me.id.parse().unwrap()
    }

    async fn wait_for(&self, what: &str, check: impl Fn(&AppState) -> bool) {
        for _ in 0..600 {
            if check(&self.service.state()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("timed out waiting for: {what}\nstate: {:#?}", self.service.state());
    }

    async fn wait_notice(&self, notice: Notice) {
        for _ in 0..100 {
            if self.notices.lock().unwrap().contains(&notice) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("never got {notice:?}; got {:?}", self.notices.lock().unwrap());
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Runs `f` on a plain thread outside the async runtime, the way a UI thread calls in.
fn off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(f).join().expect("service method panicked outside the runtime")
}

fn rand_suffix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64
}

fn contact<'a>(state: &'a AppState, id: &EndpointId) -> Option<&'a nevar_core::service::ContactView> {
    state.contacts.iter().find(|c| c.id == id.to_string())
}

/// Makes `a` and `b` friends and waits until each sees the other online.
async fn befriend(a: &Peer, b: &Peer) {
    let (a_id, b_id) = (a.id(), b.id());
    a.service.add_friend(&b_id.to_string()).unwrap();
    b.wait_for("request arrives", |s| contact(s, &a_id).is_some_and(|c| c.state == ContactState::Incoming)).await;
    b.service.accept_request(&a_id).unwrap();
    for (me, them) in [(a, b_id), (b, a_id)] {
        me.wait_for("friend online", |s| {
            contact(s, &them).is_some_and(|c| c.state == ContactState::Friend && c.online)
        })
        .await;
    }
}

/// Names of the people in my call, sorted.
fn members(state: &AppState) -> Vec<String> {
    let mut names: Vec<_> =
        state.call.iter().flat_map(|c| c.members.iter().map(|m| m.name.clone())).collect();
    names.sort();
    names
}

#[tokio::test(flavor = "multi_thread")]
async fn friends_and_call_signaling() {
    let (a, b) = tokio::join!(Peer::start("Ayse"), Peer::start("Burak"));
    a.wait_for("A online", |s| s.me.online).await;
    b.wait_for("B online", |s| s.me.online).await;
    let (a_id, b_id) = (a.id(), b.id());

    // A sends a request; B sees it with A's name.
    let svc = a.service.clone();
    off_runtime(move || svc.add_friend(&b_id.to_string())).unwrap();
    b.wait_for("request reaches B", |s| {
        contact(s, &a_id).is_some_and(|c| c.state == ContactState::Incoming && c.name == "Ayse")
    })
    .await;
    b.wait_notice(Notice::FriendRequest { name: "Ayse".into() }).await;

    // B accepts: both become online friends, with each other's names.
    let svc = b.service.clone();
    off_runtime(move || svc.accept_request(&a_id)).unwrap();
    a.wait_for("A sees B as online friend", |s| {
        contact(s, &b_id).is_some_and(|c| c.state == ContactState::Friend && c.online && c.name == "Burak")
    })
    .await;
    b.wait_for("B sees A as online friend", |s| {
        contact(s, &a_id).is_some_and(|c| c.state == ContactState::Friend && c.online)
    })
    .await;
    a.wait_notice(Notice::FriendAccepted { name: "Burak".into() }).await;

    // A rings B; B declines.
    let svc = a.service.clone();
    off_runtime(move || svc.call(&b_id)).unwrap();
    b.wait_for("B is ringing", |s| s.call.as_ref().is_some_and(|c| c.phase == CallPhase::Incoming)).await;
    b.service.decline_call();
    a.wait_for("A's ring ends", |s| s.call.is_none()).await;
    a.wait_notice(Notice::CallDeclined { name: "Burak".into() }).await;

    // A rings again and cancels before B answers: B sees a missed call.
    a.service.call(&b_id).unwrap();
    b.wait_for("B is ringing again", |s| s.call.is_some()).await;
    a.service.hang_up();
    b.wait_for("B's ring stops", |s| s.call.is_none()).await;
    b.wait_notice(Notice::MissedCall { name: "Ayse".into() }).await;

    // Renaming reaches the friend.
    b.service.set_name("Burak K").unwrap();
    a.wait_for("A sees new name", |s| contact(s, &b_id).is_some_and(|c| c.name == "Burak K")).await;

    // A removes B: gone on both sides.
    a.service.remove_contact(&b_id);
    b.wait_for("B no longer has A", |s| contact(s, &a_id).is_none()).await;
    assert!(contact(&a.service.state(), &b_id).is_none());

    tokio::join!(a.service.shutdown(), b.service.shutdown());
}

#[tokio::test(flavor = "multi_thread")]
async fn group_call_reaches_everyone_and_survives_leavers() {
    let (a, b, c) = tokio::join!(Peer::start("Ayse"), Peer::start("Burak"), Peer::start("Cem"));
    for p in [&a, &b, &c] {
        p.wait_for("online", |s| s.me.online).await;
    }
    // Ayse knows both; Burak and Cem don't know each other.
    befriend(&a, &b).await;
    befriend(&a, &c).await;
    let (b_id, c_id) = (b.id(), c.id());

    // A one-to-one call to start with.
    a.service.call(&b_id).unwrap();
    b.wait_for("B is ringing", |s| s.call.as_ref().is_some_and(|c| c.phase == CallPhase::Incoming)).await;
    b.service.accept_call().unwrap();
    a.wait_for("A talks with B", |s| members(s) == ["Burak"]).await;
    b.wait_for("B talks with A", |s| members(s) == ["Ayse"]).await;
    assert_eq!(a.service.state().call.unwrap().phase, CallPhase::Active);

    // Ayse rings Cem in. Cem sees who's already talking.
    a.service.call(&c_id).unwrap();
    c.wait_for("C is ringing with Burak listed", |s| {
        s.call.as_ref().is_some_and(|call| {
            call.phase == CallPhase::Incoming
                && call.caller.as_ref().is_some_and(|p| p.name == "Ayse")
                && call.members.iter().any(|m| m.name == "Burak")
        })
    })
    .await;
    a.wait_for("A shows Cem as ringing", |s| s.call.as_ref().is_some_and(|c| c.ringing.len() == 1)).await;

    // Cem answers and reaches Burak too, although they aren't friends.
    c.service.accept_call().unwrap();
    a.wait_for("A has both", |s| members(s) == ["Burak", "Cem"]).await;
    b.wait_for("B has both", |s| members(s) == ["Ayse", "Cem"]).await;
    c.wait_for("C has both", |s| members(s) == ["Ayse", "Burak"]).await;
    // Being in a call together doesn't make them friends.
    assert!(contact(&b.service.state(), &c_id).is_none());

    // Burak leaves; the other two carry on.
    b.service.hang_up();
    b.wait_for("B is out", |s| s.call.is_none()).await;
    a.wait_for("A has Cem left", |s| members(s) == ["Cem"]).await;
    c.wait_for("C has Ayse left", |s| members(s) == ["Ayse"]).await;

    // Ayse leaves: Cem is alone, so his call ends too.
    a.service.hang_up();
    c.wait_for("C's call ends", |s| s.call.is_none()).await;

    tokio::join!(a.service.shutdown(), b.service.shutdown(), c.service.shutdown());
}
