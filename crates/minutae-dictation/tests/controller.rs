//! Controller behaviour with fakes and a paused tokio clock.

use std::{sync::Arc, time::Duration};

use minutae_core::HotkeyMode;
use minutae_dictation::{
    CopyReason, DictationConfig, DictationController, DictationDeps, DictationEvent,
    DictationState, HudState, InsertOutcome,
    fakes::{FakeAudio, FakeInserter, FakeSpeech},
};
use tokio::{runtime::Handle, sync::mpsc::UnboundedReceiver, time::sleep};

struct Rig {
    ctl: DictationController,
    rx: UnboundedReceiver<DictationEvent>,
    audio: Arc<FakeAudio>,
    stt: Arc<FakeSpeech>,
    ins: Arc<FakeInserter>,
}

fn rig_with(stt: Arc<FakeSpeech>, outcome: InsertOutcome, config: DictationConfig) -> Rig {
    let audio = FakeAudio::one_second();
    let ins = FakeInserter::new(outcome);
    let deps = DictationDeps {
        audio: audio.clone(),
        stt: stt.clone(),
        inserter: ins.clone(),
    };
    let (ctl, rx) = DictationController::new(deps, config, Handle::current());
    Rig {
        ctl,
        rx,
        audio,
        stt,
        ins,
    }
}

fn rig(text: &str) -> Rig {
    rig_with(
        FakeSpeech::new(text),
        InsertOutcome::Inserted,
        DictationConfig::default(),
    )
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

impl Rig {
    /// Lets spawned tasks run, then returns every event emitted so far.
    async fn drain(&mut self) -> Vec<DictationEvent> {
        sleep(ms(1)).await;
        let mut out = vec![];
        while let Ok(e) = self.rx.try_recv() {
            out.push(e);
        }
        out
    }

    async fn huds(&mut self) -> Vec<HudState> {
        self.drain()
            .await
            .into_iter()
            .filter_map(|e| match e {
                DictationEvent::Hud(h) => Some(h),
                DictationEvent::Finished(_) => None,
            })
            .collect()
    }
}

fn listening(hands_free: bool) -> HudState {
    HudState::Listening {
        level: 0.0,
        hands_free,
    }
}

#[tokio::test(start_paused = true)]
async fn hold_records_transcribes_and_inserts() {
    let mut r = rig("um so i think we should ship it");
    r.ctl.key_down();
    assert_eq!(r.ctl.state(), DictationState::Listening);
    sleep(ms(1000)).await;
    r.ctl.key_up();
    let events = r.drain().await;

    let huds: Vec<_> = events
        .iter()
        .filter_map(|e| {
            if let DictationEvent::Hud(h) = e {
                Some(h.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        huds,
        vec![
            listening(false),
            HudState::Processing,
            HudState::Inserted { words: 7 }
        ]
    );

    let finished = events
        .iter()
        .find_map(|e| {
            if let DictationEvent::Finished(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(finished.dictation.text, "So I think we should ship it.");
    assert_eq!(
        finished.dictation.raw_text,
        "um so i think we should ship it"
    );
    assert_eq!(finished.dictation.target_app.as_deref(), Some("Notes"));
    assert_eq!(finished.dictation.word_count, 7);
    assert_eq!(finished.dictation.duration_ms, 1000);
    assert!(finished.dictation.inserted);
    assert!(finished.audio.is_none());

    assert_eq!(r.ins.texts(), vec!["So I think we should ship it."]);
    assert_eq!((r.audio.starts(), r.audio.stops()), (1, 1));
    assert_eq!(r.ctl.state(), DictationState::Idle);
}

#[tokio::test(start_paused = true)]
async fn short_tap_is_cancelled() {
    let mut r = rig("hello");
    r.ctl.key_down();
    sleep(ms(100)).await;
    r.ctl.key_up();
    assert_eq!(
        r.ctl.state(),
        DictationState::Listening,
        "waits for a possible second tap"
    );
    sleep(ms(400)).await;
    assert_eq!(r.huds().await, vec![listening(false), HudState::Idle]);
    assert_eq!(r.ctl.state(), DictationState::Idle);
    assert_eq!(r.stt.calls(), 0);
    assert!(r.ins.texts().is_empty());
    assert_eq!(r.audio.stops(), 1, "microphone closed");
}

#[tokio::test(start_paused = true)]
async fn double_tap_enters_hands_free_until_next_press() {
    let mut r = rig("hands free text");
    r.ctl.key_down();
    sleep(ms(100)).await;
    r.ctl.key_up();
    sleep(ms(100)).await;
    r.ctl.key_down();
    r.ctl.key_up();
    sleep(ms(3000)).await;
    assert_eq!(r.ctl.state(), DictationState::Listening);
    assert_eq!(r.huds().await, vec![listening(false), listening(true)]);

    r.ctl.key_down();
    r.ctl.key_up();
    assert_eq!(
        r.huds().await,
        vec![HudState::Processing, HudState::Inserted { words: 3 }]
    );
    assert_eq!(r.ins.texts(), vec!["Hands free text."]);
}

#[tokio::test(start_paused = true)]
async fn hold_ignores_key_repeat() {
    let mut r = rig("repeat");
    r.ctl.key_down();
    sleep(ms(300)).await;
    r.ctl.key_down();
    r.ctl.key_down();
    sleep(ms(300)).await;
    r.ctl.key_up();
    assert_eq!(
        r.huds().await,
        vec![
            listening(false),
            HudState::Processing,
            HudState::Inserted { words: 1 }
        ]
    );
    assert_eq!(r.audio.starts(), 1);
}

#[tokio::test(start_paused = true)]
async fn toggle_mode_press_starts_and_press_stops() {
    let config = DictationConfig {
        mode: HotkeyMode::Toggle,
        ..Default::default()
    };
    let mut r = rig_with(FakeSpeech::new("toggled"), InsertOutcome::Inserted, config);
    r.ctl.key_down();
    r.ctl.key_up();
    sleep(ms(2000)).await;
    assert_eq!(r.ctl.state(), DictationState::Listening);
    r.ctl.key_down();
    r.ctl.key_up();
    assert_eq!(
        r.huds().await,
        vec![
            listening(false),
            HudState::Processing,
            HudState::Inserted { words: 1 }
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn toggle_mode_very_short_session_cancels() {
    let config = DictationConfig {
        mode: HotkeyMode::Toggle,
        ..Default::default()
    };
    let mut r = rig_with(FakeSpeech::new("x"), InsertOutcome::Inserted, config);
    r.ctl.key_down();
    sleep(ms(100)).await;
    r.ctl.key_down();
    assert_eq!(r.huds().await, vec![listening(false), HudState::Idle]);
    assert_eq!(r.stt.calls(), 0);
}

#[tokio::test(start_paused = true)]
async fn max_length_stops_and_inserts() {
    let config = DictationConfig {
        max_duration: Duration::from_secs(5),
        ..Default::default()
    };
    let mut r = rig_with(FakeSpeech::new("long one"), InsertOutcome::Inserted, config);
    r.ctl.key_down();
    sleep(ms(5100)).await;
    assert_eq!(r.ctl.state(), DictationState::Idle);
    r.ctl.key_up(); // the eventual release does nothing
    assert_eq!(
        r.huds().await,
        vec![
            listening(false),
            HudState::Processing,
            HudState::Inserted { words: 2 }
        ]
    );
    assert_eq!(r.ins.texts().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn presses_while_busy_are_ignored() {
    let mut r = rig_with(
        FakeSpeech::with_delay("slow", ms(2000)),
        InsertOutcome::Inserted,
        Default::default(),
    );
    r.ctl.key_down();
    sleep(ms(500)).await;
    r.ctl.key_up();
    sleep(ms(100)).await;
    assert_eq!(r.ctl.state(), DictationState::Processing);
    r.ctl.key_down();
    r.ctl.key_up();
    assert_eq!(r.audio.starts(), 1);
    sleep(ms(2500)).await;
    assert_eq!(
        r.huds().await,
        vec![
            listening(false),
            HudState::Processing,
            HudState::Inserted { words: 1 }
        ]
    );
    assert_eq!(r.ctl.state(), DictationState::Idle);
}

#[tokio::test(start_paused = true)]
async fn transcription_error_then_recovers() {
    let mut r = rig_with(
        FakeSpeech::failing("model missing"),
        InsertOutcome::Inserted,
        Default::default(),
    );
    r.ctl.key_down();
    sleep(ms(500)).await;
    r.ctl.key_up();
    let huds = r.huds().await;
    assert_eq!(
        huds.last(),
        Some(&HudState::Error {
            message: "Couldn't transcribe: model missing".into()
        })
    );
    assert_eq!(r.ctl.state(), DictationState::Error);
    assert!(r.ins.texts().is_empty());

    r.ctl.key_down();
    assert_eq!(r.ctl.state(), DictationState::Listening);
}

#[tokio::test(start_paused = true)]
async fn copied_outcome_reaches_hud_and_record() {
    let mut r = rig_with(
        FakeSpeech::new("hello there"),
        InsertOutcome::Copied(CopyReason::NoTextField),
        Default::default(),
    );
    r.ctl.key_down();
    sleep(ms(500)).await;
    r.ctl.key_up();
    let events = r.drain().await;
    assert!(events.contains(&DictationEvent::Hud(HudState::Copied {
        words: 2,
        reason: CopyReason::NoTextField
    })));
    let finished = events
        .iter()
        .find_map(|e| {
            if let DictationEvent::Finished(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .unwrap();
    assert!(!finished.dictation.inserted);
    assert_eq!(
        finished.outcome,
        InsertOutcome::Copied(CopyReason::NoTextField)
    );
    assert_eq!(r.ctl.state(), DictationState::Idle);
}

#[tokio::test(start_paused = true)]
async fn switching_apps_while_transcribing_copies_instead_of_pasting() {
    let mut r = rig("ship it");
    r.ctl.key_down(); // in Notes
    sleep(ms(500)).await;
    r.ins.set_app(Some("Terminal"));
    r.ctl.key_up();
    let events = r.drain().await;
    assert!(events.contains(&DictationEvent::Hud(HudState::Copied {
        words: 2,
        reason: CopyReason::FocusChanged
    })));
    assert!(
        r.ins.texts().is_empty(),
        "nothing may be pasted into Terminal"
    );
    assert_eq!(r.ins.copied_texts().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn insertion_failure_is_an_error() {
    let mut r = rig_with(
        FakeSpeech::new("secret"),
        InsertOutcome::Failed("Won't type into a password field".into()),
        Default::default(),
    );
    r.ctl.key_down();
    sleep(ms(500)).await;
    r.ctl.key_up();
    let huds = r.huds().await;
    assert_eq!(
        huds.last(),
        Some(&HudState::Error {
            message: "Won't type into a password field".into()
        })
    );
    assert_eq!(r.ctl.state(), DictationState::Error);
}

#[tokio::test(start_paused = true)]
async fn silence_is_not_inserted() {
    let mut r = rig("um uh [BLANK_AUDIO]");
    r.ctl.key_down();
    sleep(ms(500)).await;
    r.ctl.key_up();
    let huds = r.huds().await;
    assert_eq!(
        huds.last(),
        Some(&HudState::Error {
            message: "No speech detected".into()
        })
    );
    assert!(r.ins.texts().is_empty());
}

#[tokio::test(start_paused = true)]
async fn cancel_while_listening_and_while_processing() {
    let mut r = rig_with(
        FakeSpeech::with_delay("never typed", ms(1000)),
        InsertOutcome::Inserted,
        Default::default(),
    );
    r.ctl.key_down();
    sleep(ms(500)).await;
    r.ctl.cancel();
    assert_eq!(r.huds().await, vec![listening(false), HudState::Idle]);

    r.ctl.key_down();
    sleep(ms(500)).await;
    r.ctl.key_up();
    sleep(ms(100)).await;
    r.ctl.cancel();
    sleep(ms(2000)).await;
    let events = r.drain().await;
    assert_eq!(
        events,
        vec![
            DictationEvent::Hud(listening(false)),
            DictationEvent::Hud(HudState::Processing),
            DictationEvent::Hud(HudState::Idle)
        ]
    );
    assert!(r.ins.texts().is_empty());
    assert_eq!(r.ctl.state(), DictationState::Idle);
}

#[tokio::test(start_paused = true)]
async fn forwards_mic_level_while_listening() {
    let mut r = rig("level");
    r.ctl.key_down();
    sleep(ms(10)).await;
    r.audio.set_level(0.5);
    sleep(ms(10)).await;
    r.audio.set_level(2.0);
    let huds = r.huds().await;
    assert_eq!(
        huds,
        vec![
            listening(false),
            HudState::Listening {
                level: 0.5,
                hands_free: false
            },
            HudState::Listening {
                level: 1.0,
                hands_free: false
            }
        ]
    );
    r.ctl.cancel();
    r.audio.set_level(0.3);
    assert_eq!(
        r.huds().await,
        vec![HudState::Idle],
        "no levels after listening ends"
    );
}

#[tokio::test(start_paused = true)]
async fn microphone_failure_is_reported() {
    let mut r = rig("x");
    *r.audio.fail_start.lock().unwrap() = true;
    r.ctl.key_down();
    assert_eq!(r.ctl.state(), DictationState::Error);
    assert_eq!(
        r.huds().await,
        vec![HudState::Error {
            message: "Microphone unavailable: no input device".into()
        }]
    );
}

#[tokio::test(start_paused = true)]
async fn vocabulary_and_audio_settings_apply() {
    let config = DictationConfig {
        vocabulary: vec!["SwiftUI".into()],
        keep_audio: true,
        ..Default::default()
    };
    let mut r = rig_with(
        FakeSpeech::new("built in swift ui"),
        InsertOutcome::Inserted,
        config,
    );
    r.ctl.key_down();
    sleep(ms(400)).await;
    r.ctl.key_up();
    let events = r.drain().await;
    let finished = events
        .iter()
        .find_map(|e| {
            if let DictationEvent::Finished(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(finished.dictation.text, "Built in SwiftUI.");
    assert_eq!(finished.audio.as_ref().map(Vec::len), Some(16_000));
    assert_eq!(
        r.stt.calls.lock().unwrap()[0],
        (16_000, vec!["SwiftUI".to_string()])
    );
}

#[test]
fn hud_state_wire_format() {
    let json = serde_json::to_value(HudState::Listening {
        level: 0.5,
        hands_free: true,
    })
    .unwrap();
    assert_eq!(
        json,
        serde_json::json!({"state": "listening", "level": 0.5, "handsFree": true})
    );
    let json = serde_json::to_value(HudState::Copied {
        words: 3,
        reason: CopyReason::Wayland,
    })
    .unwrap();
    assert_eq!(
        json,
        serde_json::json!({"state": "copied", "words": 3, "reason": "wayland"})
    );
    let json = serde_json::to_value(InsertOutcome::Failed("x".into())).unwrap();
    assert_eq!(json, serde_json::json!({"kind": "failed", "detail": "x"}));
}
