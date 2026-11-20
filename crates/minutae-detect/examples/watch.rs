//! Print mic-busy events: `cargo run -p minutae-detect --example watch -- [seconds]`

use std::time::{Duration, Instant};

fn main() -> Result<(), minutae_detect::DetectError> {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let mut probe = minutae_detect::platform_monitor()?;
    println!("initial probe: {:?}", probe.poll()?);
    let mut mon = minutae_detect::start_monitor(Duration::from_millis(500))?;
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        match mon.events.try_recv() {
            Ok(ev) => println!("{:?}", ev),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    Ok(())
}
