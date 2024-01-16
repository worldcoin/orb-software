use crate::sys::manager_event_t;

/// An event sent to the manager callback
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum Event {
    Connect,
    Disconnect,
    ReadyToPair,
    Error,
}

#[derive(Debug, Eq, PartialEq)]
pub struct UnexpectedEventError(pub manager_event_t);

impl TryFrom<manager_event_t> for Event {
    type Error = UnexpectedEventError;

    fn try_from(value: manager_event_t) -> Result<Self, UnexpectedEventError> {
        Ok(match value {
            manager_event_t::Connect => Event::Connect,
            manager_event_t::Disconnect => Event::Disconnect,
            manager_event_t::ReadyToPair => Event::ReadyToPair,
            manager_event_t::Error => Event::Error,
            _ => return Err(UnexpectedEventError(value)),
        })
    }
}
