//! Manual check: `cargo run -p minutae-dictation --example insert -- "hello world"`
//! Waits 3 s (switch to a text field), then inserts the text into the frontmost app.

use std::time::Duration;

use minutae_dictation::{
    Inserter, SystemInserter, accessibility_trusted, insertion_support, session_kind,
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let text = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let text = if text.is_empty() {
        "hello world".to_string()
    } else {
        text
    };

    println!(
        "session: {:?}, support: {:?}",
        session_kind(),
        insertion_support()
    );
    println!("accessibility trusted: {}", accessibility_trusted());
    println!("inserting {text:?} in 3 s — focus a text field now");
    tokio::time::sleep(Duration::from_secs(3)).await;

    let inserter = SystemInserter::default();
    println!(
        "paste keycode (main thread lookup): {:?}",
        minutae_dictation::refresh_paste_keycode()
    );
    println!("frontmost app: {:?}", inserter.target_app());
    let outcome = inserter.insert(&text, Default::default()).await;
    println!("outcome: {outcome:?}");
}
