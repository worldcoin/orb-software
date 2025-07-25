use crate::{
    handlers::read_file, job_system::handler::JobHandler, settings::Settings,
    shell::Shell,
};

pub struct Deps<S> {
    pub shell: S,
    pub settings: Settings,
}

pub async fn run<S>(deps: Deps<S>)
where
    S: Shell,
{
    JobHandler::builder()
        .parallel("read_file", read_file::handler)
        .build(deps)
        .run()
        .await;
}
