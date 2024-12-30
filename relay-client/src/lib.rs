//! Orb-Relay crate
use orb_relay_messages::{
    common,
    orb_commands::{self},
    prost::Name,
    prost_types::Any,
    self_serve,
};

pub mod client;

pub trait PayloadMatcher {
    type Output;
    fn matches(payload: &Any) -> Option<Self::Output>;
}

fn unpack_any<T: Name + Default>(any: &Any) -> Option<T> {
    if any.type_url != T::type_url() {
        return None;
    }
    T::decode(any.value.as_slice()).ok()
}

impl PayloadMatcher for self_serve::app::v1::StartCapture {
    type Output = self_serve::app::v1::StartCapture;

    fn matches(payload: &Any) -> Option<Self::Output> {
        if let Some(self_serve::app::v1::w::W::StartCapture(p)) =
            unpack_any::<self_serve::app::v1::W>(payload)?.w
        {
            return Some(p);
        }
        unpack_any::<Self>(payload)
    }
}

impl PayloadMatcher for self_serve::app::v1::RequestState {
    type Output = self_serve::app::v1::RequestState;

    fn matches(payload: &Any) -> Option<Self::Output> {
        if let Some(self_serve::app::v1::w::W::RequestState(p)) =
            unpack_any::<self_serve::app::v1::W>(payload)?.w
        {
            return Some(p);
        }
        unpack_any::<Self>(payload)
    }
}

impl PayloadMatcher for common::v1::AnnounceOrbId {
    type Output = common::v1::AnnounceOrbId;

    fn matches(payload: &Any) -> Option<Self::Output> {
        if let Some(common::v1::w::W::AnnounceOrbId(p)) =
            unpack_any::<common::v1::W>(payload)?.w
        {
            return Some(p);
        }
        unpack_any::<Self>(payload)
    }
}

impl PayloadMatcher for self_serve::orb::v1::SignupEnded {
    type Output = self_serve::orb::v1::SignupEnded;

    fn matches(payload: &Any) -> Option<Self::Output> {
        let w: self_serve::orb::v1::W = unpack_any(payload)?;
        match w.w {
            Some(self_serve::orb::v1::w::W::SignupEnded(p)) => Some(p),
            _ => None,
        }
    }
}

impl PayloadMatcher for orb_commands::v1::OrbCommandIssue {
    type Output = orb_commands::v1::OrbCommandIssue;

    fn matches(payload: &Any) -> Option<Self::Output> {
        unpack_any::<Self>(payload)
    }
}

impl PayloadMatcher for orb_commands::v1::OrbCommandResult {
    type Output = orb_commands::v1::OrbCommandResult;

    fn matches(payload: &Any) -> Option<Self::Output> {
        unpack_any::<Self>(payload)
    }
}

impl PayloadMatcher for orb_commands::v1::OrbCommandError {
    type Output = orb_commands::v1::OrbCommandError;

    fn matches(payload: &Any) -> Option<Self::Output> {
        unpack_any::<Self>(payload)
    }
}

pub trait IntoPayload {
    fn into_payload(self) -> Any;
}

impl IntoPayload for self_serve::orb::v1::AgeVerificationRequiredFromOperator {
    fn into_payload(self) -> Any {
        Any::from_msg(&self_serve::orb::v1::W {
            w: Some(
                self_serve::orb::v1::w::W::AgeVerificationRequiredFromOperator(self),
            ),
        })
        .unwrap()
    }
}

impl IntoPayload for self_serve::orb::v1::CaptureStarted {
    fn into_payload(self) -> Any {
        Any::from_msg(&self_serve::orb::v1::W {
            w: Some(self_serve::orb::v1::w::W::CaptureStarted(self)),
        })
        .unwrap()
    }
}

impl IntoPayload for self_serve::orb::v1::CaptureEnded {
    fn into_payload(self) -> Any {
        Any::from_msg(&self_serve::orb::v1::W {
            w: Some(self_serve::orb::v1::w::W::CaptureEnded(self)),
        })
        .unwrap()
    }
}

impl IntoPayload for self_serve::orb::v1::CaptureTriggerTimeout {
    fn into_payload(self) -> Any {
        Any::from_msg(&self_serve::orb::v1::W {
            w: Some(self_serve::orb::v1::w::W::CaptureTriggerTimeout(self)),
        })
        .unwrap()
    }
}

impl IntoPayload for self_serve::orb::v1::SignupEnded {
    fn into_payload(self) -> Any {
        Any::from_msg(&self_serve::orb::v1::W {
            w: Some(self_serve::orb::v1::w::W::SignupEnded(self)),
        })
        .unwrap()
    }
}

impl IntoPayload for self_serve::app::v1::RequestState {
    fn into_payload(self) -> Any {
        Any::from_msg(&self_serve::app::v1::W {
            w: Some(self_serve::app::v1::w::W::RequestState(self)),
        })
        .unwrap()
    }
}

impl IntoPayload for self_serve::app::v1::StartCapture {
    fn into_payload(self) -> Any {
        Any::from_msg(&self_serve::app::v1::W {
            w: Some(self_serve::app::v1::w::W::StartCapture(self)),
        })
        .unwrap()
    }
}

impl IntoPayload for common::v1::AnnounceOrbId {
    fn into_payload(self) -> Any {
        Any::from_msg(&common::v1::W {
            w: Some(common::v1::w::W::AnnounceOrbId(self)),
        })
        .unwrap()
    }
}

impl IntoPayload for common::v1::NoState {
    fn into_payload(self) -> Any {
        Any::from_msg(&common::v1::W {
            w: Some(common::v1::w::W::NoState(self)),
        })
        .unwrap()
    }
}

impl IntoPayload for orb_commands::v1::OrbCommandIssue {
    fn into_payload(self) -> Any {
        Any::from_msg(&orb_commands::v1::OrbCommandIssue {
            command_id: self.command_id,
            command: self.command,
        })
        .unwrap()
    }
}

impl IntoPayload for orb_commands::v1::OrbCommandResult {
    fn into_payload(self) -> Any {
        Any::from_msg(&orb_commands::v1::OrbCommandResult {
            command_id: self.command_id,
            result: self.result,
        })
        .unwrap()
    }
}

impl IntoPayload for orb_commands::v1::OrbCommandError {
    fn into_payload(self) -> Any {
        Any::from_msg(&orb_commands::v1::OrbCommandError { error: self.error }).unwrap()
    }
}

/// Debug any message
pub fn debug_any(any: &Option<Any>) -> String {
    let Some(any) = any else {
        return "None".to_string();
    };
    if let Some(w) = unpack_any::<self_serve::app::v1::W>(any) {
        format!("{:?}", w)
    } else if let Some(w) = unpack_any::<self_serve::orb::v1::W>(any) {
        format!("{:?}", w)
    } else if let Some(w) = unpack_any::<common::v1::W>(any) {
        format!("{:?}", w)
    } else if let Some(w) = unpack_any::<orb_commands::v1::OrbCommandIssue>(any) {
        format!("{:?}", w)
    } else if let Some(w) = unpack_any::<orb_commands::v1::OrbCommandResult>(any) {
        format!("{:?}", w)
    } else if let Some(w) = unpack_any::<orb_commands::v1::OrbCommandError>(any) {
        format!("{:?}", w)
    } else {
        "Error".to_string()
    }
}
