use super::{OrbEventStream, Payload};
use color_eyre::Result;
use zenorb::zenoh::sample::Sample;

pub(crate) async fn handler(oes: OrbEventStream, sample: Sample) -> Result<()> {
    oes.ingest(Payload::try_from(sample)?)
}
