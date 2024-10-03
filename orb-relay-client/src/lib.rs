//! Orb-Relay crate
use orb_relay_messages::{relay::relay_payload::Payload, self_serve};

pub mod client;

#[allow(missing_docs)]
pub trait PayloadMatcher {
    type Output;
    fn matches(payload: &Payload) -> Option<Self::Output>;
}

impl PayloadMatcher for self_serve::app::v1::StartCapture {
    type Output = ();

    fn matches(payload: &Payload) -> Option<Self::Output> {
        if let Payload::StartCapture(_) = payload { Some(()) } else { None }
    }
}

#[allow(missing_docs)]
pub trait IntoPayload {
    fn into_payload(self) -> Payload;
}

impl IntoPayload for self_serve::orb::v1::AgeVerificationRequiredFromOperator {
    fn into_payload(self) -> Payload {
        Payload::AgeVerificationRequiredFromOperator(self)
    }
}

impl IntoPayload for self_serve::orb::v1::CaptureStarted {
    fn into_payload(self) -> Payload {
        Payload::CaptureStarted(self)
    }
}

impl IntoPayload for self_serve::orb::v1::CaptureEnded {
    fn into_payload(self) -> Payload {
        Payload::CaptureEnded(self)
    }
}

impl IntoPayload for self_serve::orb::v1::CaptureTriggerTimeout {
    fn into_payload(self) -> Payload {
        Payload::CaptureTriggerTimeout(self)
    }
}

impl IntoPayload for self_serve::orb::v1::AnnounceOrbId {
    fn into_payload(self) -> Payload {
        Payload::AnnounceOrbId(self)
    }
}

impl IntoPayload for self_serve::orb::v1::SignupEnded {
    fn into_payload(self) -> Payload {
        Payload::SignupEnded(self)
    }
}

impl IntoPayload for self_serve::app::v1::RequestState {
    fn into_payload(self) -> Payload {
        Payload::RequestState(self)
    }
}

impl IntoPayload for self_serve::app::v1::StartCapture {
    fn into_payload(self) -> Payload {
        Payload::StartCapture(self)
    }
}
