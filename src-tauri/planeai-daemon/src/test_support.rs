use std::time::{Duration, Instant};

/// Polls `done` until it holds: a fixed sleep flakes when the whole workspace's tests load the machine.
pub fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what} within 10s");
        std::thread::sleep(Duration::from_millis(20));
    }
}
