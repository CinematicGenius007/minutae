//! In-memory stand-ins for tests and UI development.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use tokio::sync::watch;

use crate::{
    insert::{InsertOutcome, Inserter},
    source::{AudioSource, SpeechToText},
};

/// Returns `samples` from every `stop`. Push levels with [`FakeAudio::set_level`].
pub struct FakeAudio {
    pub samples: Mutex<Vec<f32>>,
    pub fail_start: Mutex<bool>,
    pub starts: Mutex<u32>,
    pub stops: Mutex<u32>,
    level: watch::Sender<f32>,
}

impl FakeAudio {
    pub fn new(samples: Vec<f32>) -> Arc<Self> {
        Arc::new(Self {
            samples: Mutex::new(samples),
            fail_start: Mutex::new(false),
            starts: Mutex::new(0),
            stops: Mutex::new(0),
            level: watch::channel(0.0).0,
        })
    }

    /// One second of silence at 16 kHz.
    pub fn one_second() -> Arc<Self> {
        Self::new(vec![0.0; 16_000])
    }

    pub fn set_level(&self, level: f32) {
        self.level.send_replace(level);
    }

    pub fn starts(&self) -> u32 {
        *self.starts.lock().unwrap()
    }

    pub fn stops(&self) -> u32 {
        *self.stops.lock().unwrap()
    }
}

impl AudioSource for FakeAudio {
    fn start(&self) -> anyhow::Result<()> {
        if *self.fail_start.lock().unwrap() {
            anyhow::bail!("no input device");
        }
        *self.starts.lock().unwrap() += 1;
        Ok(())
    }

    fn stop(&self) -> anyhow::Result<Vec<f32>> {
        *self.stops.lock().unwrap() += 1;
        Ok(self.samples.lock().unwrap().clone())
    }

    fn levels(&self) -> watch::Receiver<f32> {
        self.level.subscribe()
    }
}

/// Returns a fixed transcript (or error) after an optional delay.
pub struct FakeSpeech {
    pub result: Mutex<Result<String, String>>,
    pub delay: Duration,
    pub calls: Mutex<Vec<(usize, Vec<String>)>>,
}

impl FakeSpeech {
    pub fn new(text: &str) -> Arc<Self> {
        Self::with_delay(text, Duration::ZERO)
    }

    pub fn with_delay(text: &str, delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(Ok(text.into())),
            delay,
            calls: Mutex::new(vec![]),
        })
    }

    pub fn failing(message: &str) -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(Err(message.into())),
            delay: Duration::ZERO,
            calls: Mutex::new(vec![]),
        })
    }

    pub fn calls(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[async_trait]
impl SpeechToText for FakeSpeech {
    async fn transcribe(&self, samples: &[f32], vocabulary: &[String]) -> anyhow::Result<String> {
        self.calls
            .lock()
            .unwrap()
            .push((samples.len(), vocabulary.to_vec()));
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        self.result
            .lock()
            .unwrap()
            .clone()
            .map_err(anyhow::Error::msg)
    }
}

/// Records inserted text and returns a fixed outcome.
pub struct FakeInserter {
    pub outcome: Mutex<InsertOutcome>,
    pub app: Option<String>,
    pub inserted: Mutex<Vec<String>>,
}

impl FakeInserter {
    pub fn new(outcome: InsertOutcome) -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(outcome),
            app: Some("Notes".into()),
            inserted: Mutex::new(vec![]),
        })
    }

    pub fn texts(&self) -> Vec<String> {
        self.inserted.lock().unwrap().clone()
    }
}

#[async_trait]
impl Inserter for FakeInserter {
    fn target_app(&self) -> Option<String> {
        self.app.clone()
    }

    async fn insert(&self, text: &str) -> InsertOutcome {
        self.inserted.lock().unwrap().push(text.to_owned());
        self.outcome.lock().unwrap().clone()
    }
}
