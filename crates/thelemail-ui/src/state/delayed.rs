use std::time::{Duration, Instant};

pub const INDICATOR_DELAY: Duration = Duration::from_millis(300);
pub const INDICATOR_MIN: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq)]
enum Pending<K> {
    Show { key: K, at: Instant },
    Hide { at: Instant },
}

impl<K> Pending<K> {
    fn at(&self) -> Instant {
        match self {
            Self::Show { at, .. } | Self::Hide { at } => *at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Delayed<K = ()> {
    delay: Duration,
    min_visible: Duration,
    active: bool,
    key: Option<K>,
    shown: Option<(K, Instant)>,
    pending: Option<Pending<K>>,
}

impl<K: PartialEq + Clone> Default for Delayed<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: PartialEq + Clone> Delayed<K> {
    pub fn new() -> Self {
        Self::with_timing(INDICATOR_DELAY, INDICATOR_MIN)
    }

    pub fn with_timing(delay: Duration, min_visible: Duration) -> Self {
        Self {
            delay,
            min_visible,
            active: false,
            key: None,
            shown: None,
            pending: None,
        }
    }

    pub fn update(&mut self, active: bool, key: K, now: Instant) -> Option<Instant> {
        self.fire(now);
        self.active = active;
        self.key = Some(key);
        self.schedule(now);
        if self.pending.as_ref().is_some_and(|p| p.at() <= now) {
            self.fire(now);
            self.schedule(now);
        }
        self.pending.as_ref().map(Pending::at)
    }

    pub fn visible(&self) -> bool {
        self.showing()
    }

    fn showing(&self) -> bool {
        match (&self.shown, &self.key) {
            (Some((shown, _)), Some(key)) => shown == key,
            _ => false,
        }
    }

    fn fire(&mut self, now: Instant) {
        let Some(pending) = self.pending.take_if(|p| p.at() <= now) else {
            return;
        };
        match pending {
            Pending::Show { key, .. } => self.shown = Some((key, now)),
            Pending::Hide { .. } => self.shown = None,
        }
    }

    fn schedule(&mut self, now: Instant) {
        let showing = self.showing();
        if self.active && !showing {
            let key = self.key.clone().expect("key is set before scheduling");
            let at = match &self.pending {
                Some(Pending::Show { key: k, at }) if *k == key => *at,
                _ => now + self.delay,
            };
            self.pending = Some(Pending::Show { key, at });
            return;
        }
        if !self.active && self.shown.is_some() {
            if !showing {
                self.shown = None;
                self.pending = None;
                return;
            }
            let at = match &self.pending {
                Some(Pending::Hide { at }) => *at,
                _ => {
                    let since = self.shown.as_ref().map(|(_, at)| *at).unwrap_or(now);
                    (since + self.min_visible).max(now)
                }
            };
            self.pending = Some(Pending::Hide { at });
            return;
        }
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_fast_load_never_shows_the_indicator() {
        let t0 = Instant::now();
        let mut d = Delayed::<()>::new();
        assert_eq!(d.update(true, (), t0), Some(t0 + ms(300)));
        assert!(!d.visible());
        assert_eq!(d.update(false, (), t0 + ms(299)), None);
        assert!(!d.visible());
        assert_eq!(d.update(false, (), t0 + ms(400)), None);
        assert!(!d.visible());
    }

    #[test]
    fn a_slow_load_shows_after_the_delay() {
        let t0 = Instant::now();
        let mut d = Delayed::<()>::new();
        d.update(true, (), t0);
        d.update(true, (), t0 + ms(299));
        assert!(!d.visible());
        assert_eq!(d.update(true, (), t0 + ms(300)), None);
        assert!(d.visible());
    }

    #[test]
    fn repeated_updates_do_not_push_the_delay_back() {
        let t0 = Instant::now();
        let mut d = Delayed::<()>::new();
        d.update(true, (), t0);
        assert_eq!(d.update(true, (), t0 + ms(200)), Some(t0 + ms(300)));
    }

    #[test]
    fn once_shown_it_stays_for_the_minimum() {
        let t0 = Instant::now();
        let mut d = Delayed::<()>::new();
        d.update(true, (), t0);
        d.update(true, (), t0 + ms(300));
        assert_eq!(d.update(false, (), t0 + ms(350)), Some(t0 + ms(550)));
        assert!(d.visible());
        d.update(false, (), t0 + ms(549));
        assert!(d.visible());
        assert_eq!(d.update(false, (), t0 + ms(550)), None);
        assert!(!d.visible());
    }

    #[test]
    fn it_hides_at_once_when_the_minimum_already_passed() {
        let t0 = Instant::now();
        let mut d = Delayed::<()>::new();
        d.update(true, (), t0);
        d.update(true, (), t0 + ms(300));
        assert_eq!(d.update(false, (), t0 + ms(900)), None);
        assert!(!d.visible());
    }

    #[test]
    fn becoming_active_again_cancels_the_pending_hide() {
        let t0 = Instant::now();
        let mut d = Delayed::<()>::new();
        d.update(true, (), t0);
        d.update(true, (), t0 + ms(300));
        d.update(false, (), t0 + ms(350));
        assert_eq!(d.update(true, (), t0 + ms(400)), None);
        d.update(true, (), t0 + ms(800));
        assert!(d.visible());
    }

    #[test]
    fn a_key_change_hides_at_once_and_restarts_the_delay() {
        let t0 = Instant::now();
        let mut d = Delayed::<&str>::new();
        d.update(true, "inbox", t0);
        d.update(true, "inbox", t0 + ms(300));
        assert!(d.visible());
        assert_eq!(d.update(true, "sent", t0 + ms(320)), Some(t0 + ms(620)));
        assert!(!d.visible());
        d.update(true, "sent", t0 + ms(620));
        assert!(d.visible());
    }

    #[test]
    fn going_idle_under_another_key_clears_without_waiting() {
        let t0 = Instant::now();
        let mut d = Delayed::<&str>::new();
        d.update(true, "inbox", t0);
        d.update(true, "inbox", t0 + ms(300));
        assert_eq!(d.update(false, "sent", t0 + ms(310)), None);
        assert!(!d.visible());
        d.update(false, "inbox", t0 + ms(320));
        assert!(!d.visible());
    }

    #[test]
    fn custom_timing_is_respected() {
        let t0 = Instant::now();
        let mut d = Delayed::<()>::with_timing(ms(10), ms(5));
        assert_eq!(d.update(true, (), t0), Some(t0 + ms(10)));
        d.update(true, (), t0 + ms(10));
        assert_eq!(d.update(false, (), t0 + ms(11)), Some(t0 + ms(15)));
    }
}
