use tracing::metadata::LevelFilter;
use tracing_subscriber::{
    filter::EnvFilter,
    fmt::MakeWriter,
};

pub fn start<W>(env_filter: LevelFilter, sink: W)
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .with_writer(sink)
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(env_filter.into())
                .from_env_lossy(),
        )
        .init();
}
