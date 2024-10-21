use std::{
    ops::Deref,
    time::{Duration, Instant},
};

use crate::ONE_DAY;

#[derive(Debug, Clone)]
pub struct State {
    state: String,
    retrieved_at: Instant,
    expires_in: Option<Duration>,
}

impl State {
    pub fn new(state: String, expires_in: Option<Duration>) -> Self {
        Self {
            state,
            expires_in,
            retrieved_at: Instant::now(),
        }
    }

    /// The time that the state was retrieved from the backend.
    pub fn retrieved_at(&self) -> Instant {
        self.retrieved_at
    }

    /// Whether the state is expried, and should be requested from the backend again.
    pub fn is_expired(&self) -> bool {
        self.retrieved_at.elapsed() >= self.expires_in.unwrap_or(Duration::MAX)
    }

    /// The time at which the state will be expired.
    pub fn expires_at(&self) -> Instant {
        self.expires_in
            .map(|d| self.retrieved_at + d)
            .unwrap_or(self.retrieved_at + ONE_DAY)
    }
}

impl Deref for State {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl From<State> for String {
    fn from(value: State) -> Self {
        value.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expires_at() {
        let no_expire = State::new("foo".into(), None);
        let expiry0 = Duration::from_secs(0);
        let expiry1 = Duration::from_secs(2);
        let expires0 = State::new("2".into(), Some(expiry0));
        assert!(expires0.is_expired());
        let expires1 = State::new("1".into(), Some(expiry1));
        assert!(!expires1.is_expired());

        assert_eq!(expires1.expires_at(), expires1.retrieved_at() + expiry1);
        assert_eq!(expires0.expires_at(), expires0.retrieved_at() + expiry0);
        assert_eq!(no_expire.expires_at(), no_expire.retrieved_at() + ONE_DAY);
    }
}
