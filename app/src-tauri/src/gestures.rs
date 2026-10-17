//! Turns raw down/up events into press, long press and double press.
//! A button only waits (to tell gestures apart) when it has a long or double action;
//! otherwise it fires on the way down, with no added delay.
//! Timers are deadlines: the deck loop sleeps until `next_deadline` and calls `tick`.

use crate::config::Gesture;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Which of long / double the button has actions for right now.
#[derive(Clone, Copy, Default)]
pub struct Uses {
    pub long: bool,
    pub double: bool,
}

pub struct Gestures {
    pub long_press: Duration,
    pub double_gap: Duration,
    uses: HashMap<u8, Uses>,      // gestures in play for a button that's down
    long_at: HashMap<u8, Instant>,
    pending: HashMap<u8, Instant>, // single press waiting to see if a second one comes
    handled: HashSet<u8>,          // this down already fired; ignore its release
}

impl Default for Gestures {
    fn default() -> Self {
        Gestures {
            long_press: Duration::from_millis(500),
            double_gap: Duration::from_millis(300),
            uses: HashMap::new(),
            long_at: HashMap::new(),
            pending: HashMap::new(),
            handled: HashSet::new(),
        }
    }
}

impl Gestures {
    pub fn down(&mut self, n: u8, uses: Uses, now: Instant) -> Option<Gesture> {
        if self.pending.remove(&n).is_some() {
            self.handled.insert(n);
            return Some(Gesture::Double);
        }
        if !uses.long && !uses.double {
            self.handled.insert(n);
            return Some(Gesture::Press);
        }
        self.uses.insert(n, uses);
        if uses.long {
            self.long_at.insert(n, now + self.long_press);
        }
        None
    }

    pub fn up(&mut self, n: u8, now: Instant) -> Option<Gesture> {
        self.long_at.remove(&n);
        let uses = self.uses.remove(&n);
        if self.handled.remove(&n) {
            return None;
        }
        let uses = uses?; // release of a press we never saw (port opened mid-hold)
        if uses.double {
            self.pending.insert(n, now + self.double_gap);
            None
        } else {
            Some(Gesture::Press)
        }
    }

    /// Gestures whose deadline has passed.
    pub fn tick(&mut self, now: Instant) -> Vec<(u8, Gesture)> {
        let mut fired = Vec::new();
        let due = |m: &HashMap<u8, Instant>| m.iter().filter(|(_, &at)| at <= now).map(|(&n, _)| n).collect::<Vec<_>>();
        for n in due(&self.long_at) {
            self.long_at.remove(&n);
            self.handled.insert(n);
            fired.push((n, Gesture::Long));
        }
        for n in due(&self.pending) {
            self.pending.remove(&n);
            fired.push((n, Gesture::Press));
        }
        fired
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.long_at.values().chain(self.pending.values()).min().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Gesture::*;

    const NONE: Uses = Uses { long: false, double: false };
    const LONG: Uses = Uses { long: true, double: false };
    const DOUBLE: Uses = Uses { long: false, double: true };
    const BOTH: Uses = Uses { long: true, double: true };

    struct Clock {
        g: Gestures,
        now: Instant,
        fired: Vec<Gesture>,
    }

    impl Clock {
        fn new() -> Self {
            Clock { g: Gestures::default(), now: Instant::now(), fired: vec![] }
        }
        fn down(&mut self, n: u8, uses: Uses) {
            self.fired.extend(self.g.down(n, uses, self.now));
        }
        fn up(&mut self, n: u8) {
            self.fired.extend(self.g.up(n, self.now));
        }
        fn advance(&mut self, secs: f64) {
            self.now += Duration::from_secs_f64(secs);
            self.fired.extend(self.g.tick(self.now).into_iter().map(|(_, g)| g));
        }
        fn take(&mut self) -> Vec<Gesture> {
            std::mem::take(&mut self.fired)
        }
    }

    #[test]
    fn no_gesture_actions_fire_on_the_way_down() {
        let mut c = Clock::new();
        c.down(1, NONE);
        assert_eq!(c.take(), [Press]);
        c.up(1);
        c.advance(1.0);
        assert_eq!(c.take(), []);
    }

    #[test]
    fn long_press_fires_while_held() {
        let mut c = Clock::new();
        c.down(1, LONG);
        c.advance(0.6);
        assert_eq!(c.take(), [Long]);
        c.up(1);
        c.advance(1.0);
        assert_eq!(c.take(), []);
    }

    #[test]
    fn short_tap_with_long_action_presses_on_release() {
        let mut c = Clock::new();
        c.down(1, LONG);
        c.advance(0.2);
        c.up(1);
        assert_eq!(c.take(), [Press]);
        c.advance(1.0);
        assert_eq!(c.take(), []);
    }

    #[test]
    fn double_press() {
        let mut c = Clock::new();
        c.down(1, DOUBLE);
        c.up(1);
        c.advance(0.1);
        c.down(1, DOUBLE);
        assert_eq!(c.take(), [Double]);
        c.up(1);
        c.advance(1.0);
        assert_eq!(c.take(), []);
    }

    #[test]
    fn single_tap_with_double_action_presses_after_gap() {
        let mut c = Clock::new();
        c.down(1, BOTH);
        c.up(1);
        assert_eq!(c.take(), []);
        assert!(c.g.next_deadline().is_some());
        c.advance(0.4);
        assert_eq!(c.take(), [Press]);
        assert!(c.g.next_deadline().is_none());
    }

    #[test]
    fn buttons_are_independent() {
        let mut c = Clock::new();
        c.down(1, LONG);
        c.down(2, NONE);
        assert_eq!(c.take(), [Press]);
        c.advance(0.6);
        assert_eq!(c.take(), [Long]);
    }

    #[test]
    fn release_without_press_is_ignored() {
        let mut c = Clock::new();
        c.up(3);
        c.advance(1.0);
        assert_eq!(c.take(), []);
    }
}
