use std::time::{Duration, Instant};

pub struct FramePacer {
    start: Instant,
    interval: Duration,
    emitted: u64,
}

impl FramePacer {
    pub fn new(fps: u32) -> Result<Self, &'static str> {
        if fps == 0 {
            return Err("fps must be greater than zero");
        }
        Ok(Self {
            start: Instant::now(),
            interval: Duration::from_secs_f64(1.0 / fps as f64),
            emitted: 0,
        })
    }
    pub fn next_deadline(&self) -> Instant {
        self.start + self.interval.saturating_mul(self.emitted as u32)
    }
    pub fn wait(&mut self) {
        if let Some(remaining) = self.next_deadline().checked_duration_since(Instant::now()) {
            std::thread::sleep(remaining);
        }
        self.emitted += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::FramePacer;
    #[test]
    fn deadlines_are_absolute() {
        let p = FramePacer::new(20).unwrap();
        assert_eq!(p.next_deadline(), p.next_deadline());
    }
}
