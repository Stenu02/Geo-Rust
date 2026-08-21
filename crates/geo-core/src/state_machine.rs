//! User state machine (R5, R6, R7).
//!
//! ```text
//! DISCONNECTED --login--> STOPPED --coordinate change--> MOVING
//!      ^                    ^                               |
//!      |                    +---- 3 min without change ------+
//!      |
//!      +---- disconnect (from any state)
//! ```
//!
//! CAREFUL WITH THE R7 IMPLEMENTATION: the transition to STOPPED must be based on a
//! TIMER (instant of the last actual change), not on counting samples.
//! If the client stops transmitting, the 6 samples never arrive and with the
//! counter approach it would NEVER transition to STOPPED.

use crate::position::Position;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::{stop_threshold};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UserState {
    /// Not connected.
    Disconnected,
    /// Connected, coordinates unchanged.
    Stopped,
    /// Connected, coordinates changing.
    Moving,
}

impl UserState {
    /// True only for `Stopped`. Used by the analytics for the "pause duration" (R10).
    ///
    pub fn counts_as_pause(&self) -> bool {
        matches!(self, UserState::Stopped)
    }
}

/// A state change that happened at a given instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    pub from: UserState,
    pub to: UserState,
    pub at: DateTime<Utc>,
}

/// State of a single user. One instance per connected user, held by the server.
#[derive(Debug, Clone)]
pub struct StateMachine {
    state: UserState,
    last_position: Option<Position>,
    /// Instant of the last ACTUAL coordinate change.
    last_change_at: Option<DateTime<Utc>>,
}

impl StateMachine {
    /// Builds a machine in the DISCONNECTED state.
    pub fn new() -> Self {
        Self {
            state: UserState::Disconnected,
            last_position: None,
            last_change_at: None,
        }
    }

    pub fn state(&self) -> UserState {
        self.state
    }

    pub fn last_position(&self) -> Option<Position> {
        self.last_position
    }

    /// Sets the new state and returns the corresponding transition.
    ///
    /// The four `on_*` methods use this so they only carry the DECISION
    /// (which state to move to, and at which instant), never the field bookkeeping.
    /// `at` is passed explicitly because it is not always `now`: `on_tick` closes
    /// the movement at `last_change_at + threshold` (R7).
    fn transition_to(&mut self, to: UserState, at: DateTime<Utc>) -> Transition {
        let from = self.state;
        self.state = to;
        Transition { from, to, at }
    }

    /// Login: DISCONNECTED -> STOPPED.
    ///
    /// The initial state is STOPPED, not MOVING: it switches to MOVING only
    /// on the first actual coordinate change.
    pub fn on_login(&mut self, now: DateTime<Utc>) -> Option<Transition> {
        if self.state != UserState::Disconnected {
            return None;
        }
        self.last_position = None;
        self.last_change_at = None;
        Some(self.transition_to(UserState::Stopped, now))
    }

    /// Position received from the client (R4).
    ///
    /// Cases to handle:
    /// - FIRST position after login: it is not a "change", it does not trigger R6
    /// - coordinate different from the previous one: STOPPED -> MOVING, and in any
    ///   case updates `last_change_at`
    /// - same coordinate: no transition here (`on_tick` takes care of it)
    pub fn on_position(
        &mut self,
        now: DateTime<Utc>,
        position: Position,
    ) -> Option<Transition> {
        match self.state {
            UserState::Stopped => {
                if self.last_position.is_none() {
                    self.last_position = Some(position);
                    None
                }
                else if self.last_position.unwrap().differs_from(&position) {
                    self.last_position = Some(position);
                    self.last_change_at = Some(now);
                    Some(self.transition_to(UserState::Moving, now))
                }
                else {
                    None
                }
            }
            UserState::Moving => {
                if let Some(prev) =self.last_position{
                    if prev.differs_from(&position) {
                        self.last_position = Some(position);
                        self.last_change_at = Some(now);
                    }
                }
                None
            }
            _ => None
        }
    }

    /// Periodic evaluation (R7). Called by the server at regular intervals.
    ///
    /// If the state is MOVING and at least `STOP_THRESHOLD_SECS` have passed
    /// since the last coordinate change, it transitions to STOPPED.
    ///
    /// The transition instant MUST be `last_change_at + threshold`, not `now`:
    /// otherwise the movement duration comes out longer than it should by however
    /// fine the tick is, and the R10 totals do not add up.
    pub fn on_tick(&mut self, now: DateTime<Utc>) -> Option<Transition> {
        if self.state != UserState::Moving {
            return None;
        }
        if let Some(last_change_at) = self.last_change_at {
            let delta = now - last_change_at;
            if delta >= stop_threshold() {
                return Some(self.transition_to(UserState::Stopped, last_change_at + stop_threshold() ))
            }
        }
        None
    }

    /// Disconnect from any state.
    ///
    /// On reconnect the 3-minute timer restarts: a choice to document in the report.
    pub fn on_disconnect(&mut self, now: DateTime<Utc>) -> Option<Transition> {
        if self.state == UserState::Disconnected {
            return None
        }
        self.last_position = None;
        self.last_change_at = None;
        Some(self.transition_to(UserState::Disconnected, now))
    }
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stop_threshold;
    use chrono::{Duration, TimeZone};

    /// A fixed base instant plus an offset in seconds. Time is built by hand:
    /// no `Utc::now()`, so a full trip is replayed deterministically.
    fn t(offset_secs: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap() + Duration::seconds(offset_secs)
    }

    #[test]
    fn test_state_transitions() {
        let mut sm = StateMachine::new();
        assert_eq!(sm.state(), UserState::Disconnected);

        // login: DISCONNECTED -> STOPPED
        let tr = sm.on_login(t(0)).unwrap();
        assert_eq!((tr.from, tr.to), (UserState::Disconnected, UserState::Stopped));

        // first fix: not a change, stays STOPPED
        assert!(sm.on_position(t(30), Position::new(45.0, 7.0)).is_none());
        assert_eq!(sm.state(), UserState::Stopped);

        // coordinate change: STOPPED -> MOVING
        let tr = sm.on_position(t(60), Position::new(45.001, 7.0)).unwrap();
        assert_eq!((tr.from, tr.to), (UserState::Stopped, UserState::Moving));

        // 3 min without change: MOVING -> STOPPED
        let tr = sm.on_tick(t(60) + stop_threshold() + Duration::seconds(5)).unwrap();
        assert_eq!((tr.from, tr.to), (UserState::Moving, UserState::Stopped));

        // disconnect: STOPPED -> DISCONNECTED, keeping the real `from`
        let tr = sm.on_disconnect(t(400)).unwrap();
        assert_eq!((tr.from, tr.to), (UserState::Stopped, UserState::Disconnected));
        assert_eq!(sm.state(), UserState::Disconnected);
    }

    #[test]
    fn test_first_coord_change_starts_movement() {
        let mut sm = StateMachine::new();
        sm.on_login(t(0));

        // R6: the first position after login is not a "change"
        assert!(sm.on_position(t(30), Position::new(45.0, 7.0)).is_none());
        assert_eq!(sm.state(), UserState::Stopped);

        // same coordinate again: still no movement
        assert!(sm.on_position(t(60), Position::new(45.0, 7.0)).is_none());
        assert_eq!(sm.state(), UserState::Stopped);

        // first actual change: now it moves
        let tr = sm.on_position(t(90), Position::new(45.002, 7.0)).unwrap();
        assert_eq!(tr.to, UserState::Moving);
    }

    #[test]
    fn test_three_minutes_triggers_stopped() {
        let mut sm = StateMachine::new();
        sm.on_login(t(0));
        sm.on_position(t(30), Position::new(45.0, 7.0)); // first fix
        sm.on_position(t(60), Position::new(45.001, 7.0)); // -> MOVING, last change at t(60)

        // one second before the threshold: no transition
        assert!(sm
            .on_tick(t(60) + stop_threshold() - Duration::seconds(1))
            .is_none());
        assert_eq!(sm.state(), UserState::Moving);

        // exactly at the threshold: transition to STOPPED
        let tr = sm.on_tick(t(60) + stop_threshold()).unwrap();
        assert_eq!(tr.to, UserState::Stopped);
        // the instant is last_change_at + threshold, NOT `now` (R7)
        assert_eq!(tr.at, t(60) + stop_threshold());
    }

    #[test]
    fn test_continued_movement_delays_stop() {
        // Guards the R7 timer refresh: while MOVING, each real change restarts the clock.
        let mut sm = StateMachine::new();
        sm.on_login(t(0));
        sm.on_position(t(30), Position::new(45.0, 7.0)); // first fix
        sm.on_position(t(60), Position::new(45.001, 7.0)); // -> MOVING, last change at t(60)

        // still moving at t(120): refreshes the timer, no transition
        assert!(sm.on_position(t(120), Position::new(45.002, 7.0)).is_none());
        assert_eq!(sm.state(), UserState::Moving);

        // 3 min after the FIRST change but only 2 min after the last: still MOVING
        assert!(sm.on_tick(t(60) + stop_threshold()).is_none());
        assert_eq!(sm.state(), UserState::Moving);

        // 3 min after the LAST change: now it stops, at last_change_at + threshold
        let tr = sm.on_tick(t(120) + stop_threshold()).unwrap();
        assert_eq!(tr.to, UserState::Stopped);
        assert_eq!(tr.at, t(120) + stop_threshold());
    }
}
