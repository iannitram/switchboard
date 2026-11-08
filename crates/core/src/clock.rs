//! An injected clock (SPEC.md, Testing decisions): the 10-minute poll (GH-1), reading ages,
//! the priority copy's age (SKY-9) and the Sunday 6am weekly run (AUTO-5) all read time from
//! here, so tests can move it.

/// Seconds since the Unix epoch, UTC.
pub type UnixSeconds = i64;

pub trait Clock: Send + Sync {
    fn now(&self) -> UnixSeconds;
}

/// The real clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UnixSeconds {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

/// A clock tests set by hand.
pub struct FixedClock(pub std::sync::atomic::AtomicI64);

impl FixedClock {
    pub fn at(t: UnixSeconds) -> Self {
        FixedClock(std::sync::atomic::AtomicI64::new(t))
    }
    pub fn advance(&self, secs: i64) {
        self.0.fetch_add(secs, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now(&self) -> UnixSeconds {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}
