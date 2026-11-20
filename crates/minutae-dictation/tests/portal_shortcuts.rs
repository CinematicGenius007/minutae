//! Portal shortcut signals (fake, no D-Bus) driven through the router into the real
//! dictation controller: hold works when releases arrive, toggle when they don't.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
};
use std::time::Duration;

use minutae_core::HotkeyMode;
use minutae_dictation::{
    DictationConfig, DictationController, DictationDeps, DictationState, InsertOutcome, KeySink,
    PortalError, ReleaseSupport, ShortcutId, ShortcutPortal, ShortcutRequest, ShortcutSignal,
    fakes::{FakeAudio, FakeInserter, FakeSpeech},
    run_router,
    shortcuts::FakePortal,
};
use tokio::{runtime::Handle, time::sleep};

struct Sink {
    ctl: DictationController,
    chosen: HotkeyMode,
    support: Mutex<Vec<ReleaseSupport>>,
    records: AtomicU32,
}

impl KeySink for Sink {
    fn release_support(&self, support: ReleaseSupport) {
        self.support.lock().unwrap().push(support);
        let mut c = self.ctl.config();
        c.mode = support.effective_mode(self.chosen);
        self.ctl.set_config(c);
    }
    fn dictate(&self, down: bool) {
        if down {
            self.ctl.key_down()
        } else {
            self.ctl.key_up()
        }
    }
    fn toggle_recording(&self) {
        self.records.fetch_add(1, Ordering::SeqCst);
    }
}

struct Rig {
    _events: tokio::sync::mpsc::UnboundedReceiver<minutae_dictation::DictationEvent>,
    portal: Arc<FakePortal>,
    sink: Arc<Sink>,
    ins: Arc<FakeInserter>,
}

async fn rig() -> Rig {
    let ins = FakeInserter::new(InsertOutcome::Inserted);
    let deps = DictationDeps {
        audio: FakeAudio::one_second(),
        stt: FakeSpeech::new("hello there"),
        inserter: ins.clone(),
    };
    let (ctl, events) =
        DictationController::new(deps, DictationConfig::default(), Handle::current());
    let sink = Arc::new(Sink {
        ctl,
        chosen: HotkeyMode::Hold,
        support: Mutex::new(vec![]),
        records: AtomicU32::new(0),
    });
    let portal = FakePortal::new(true);
    let binding = portal
        .bind(&[
            ShortcutRequest {
                id: ShortcutId::Dictate,
                description: "Dictate".into(),
                preferred_trigger: Some("ALT+space".into()),
            },
            ShortcutRequest {
                id: ShortcutId::Record,
                description: "Record".into(),
                preferred_trigger: Some("CTRL+ALT+r".into()),
            },
        ])
        .await
        .unwrap();
    assert_eq!(binding.shortcuts.len(), 2);
    assert_eq!(binding.shortcuts[0].trigger, "ALT+space");
    tokio::spawn(run_router(binding.signals, sink.clone()));
    Rig {
        _events: events,
        portal,
        sink,
        ins,
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn act(id: ShortcutId, at_ms: u64) -> ShortcutSignal {
    ShortcutSignal::Activated { id, at_ms }
}
fn deact(id: ShortcutId, at_ms: u64) -> ShortcutSignal {
    ShortcutSignal::Deactivated { id, at_ms }
}

#[tokio::test(start_paused = true)]
async fn hold_to_talk_with_activated_and_deactivated() {
    let r = rig().await;
    r.portal.send(act(ShortcutId::Dictate, 0));
    sleep(ms(1000)).await;
    assert_eq!(r.sink.ctl.state(), DictationState::Listening);
    r.portal.send(deact(ShortcutId::Dictate, 1000));
    sleep(ms(5)).await;
    assert_eq!(r.ins.texts(), vec!["Hello there."]);
    assert_eq!(
        *r.sink.support.lock().unwrap(),
        vec![ReleaseSupport::Reported]
    );
    assert_eq!(r.sink.ctl.config().mode, HotkeyMode::Hold);
}

#[tokio::test(start_paused = true)]
async fn activated_only_falls_back_to_toggle() {
    let r = rig().await;
    // First press starts; no release ever comes.
    r.portal.send(act(ShortcutId::Dictate, 0));
    sleep(ms(2000)).await;
    assert_eq!(r.sink.ctl.state(), DictationState::Listening);
    // Second press: the router learns releases are missing and stops the dictation.
    r.portal.send(act(ShortcutId::Dictate, 2000));
    sleep(ms(5)).await;
    assert_eq!(r.ins.texts(), vec!["Hello there."]);
    assert_eq!(
        *r.sink.support.lock().unwrap(),
        vec![ReleaseSupport::Missing]
    );
    assert_eq!(r.sink.ctl.config().mode, HotkeyMode::Toggle);
    // From now on each press toggles.
    r.portal.send(act(ShortcutId::Dictate, 5000));
    sleep(ms(1500)).await;
    assert_eq!(r.sink.ctl.state(), DictationState::Listening);
    r.portal.send(act(ShortcutId::Dictate, 6500));
    sleep(ms(5)).await;
    assert_eq!(r.ins.texts().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn instant_deactivated_is_treated_as_no_release() {
    let r = rig().await;
    r.portal.send(act(ShortcutId::Dictate, 10));
    r.portal.send(deact(ShortcutId::Dictate, 11));
    sleep(ms(1500)).await;
    // Still listening: the synthetic release did not end (or cancel) it.
    assert_eq!(r.sink.ctl.state(), DictationState::Listening);
    assert_eq!(r.sink.ctl.config().mode, HotkeyMode::Toggle);
    r.portal.send(act(ShortcutId::Dictate, 1500));
    r.portal.send(deact(ShortcutId::Dictate, 1501));
    sleep(ms(5)).await;
    assert_eq!(r.ins.texts(), vec!["Hello there."]);
}

#[tokio::test(start_paused = true)]
async fn record_shortcut_toggles_recording_on_press() {
    let r = rig().await;
    r.portal.send(act(ShortcutId::Record, 0));
    r.portal.send(deact(ShortcutId::Record, 300));
    r.portal.send(act(ShortcutId::Record, 5000));
    sleep(ms(5)).await;
    assert_eq!(r.sink.records.load(Ordering::SeqCst), 2);
    assert_eq!(r.sink.ctl.state(), DictationState::Idle);
    r.portal.close();
}

#[tokio::test]
async fn unavailable_portal_reports_unavailable_and_configure() {
    let p = FakePortal::unavailable();
    let err = p.bind(&[]).await.err().unwrap();
    assert!(matches!(err, PortalError::Unavailable(_)));
    let p = FakePortal::new(true);
    p.configure().await.unwrap();
    assert_eq!(*p.configured.lock().unwrap(), 1);
    assert!(FakePortal::new(false).configure().await.is_err());
}

#[tokio::test(start_paused = true)]
async fn toggle_entry_point_starts_and_stops_in_hold_mode() {
    let r = rig().await;
    r.sink.ctl.toggle();
    sleep(ms(1000)).await;
    assert_eq!(r.sink.ctl.state(), DictationState::Listening);
    r.sink.ctl.toggle();
    sleep(ms(5)).await;
    assert_eq!(r.ins.texts(), vec!["Hello there."]);
}
