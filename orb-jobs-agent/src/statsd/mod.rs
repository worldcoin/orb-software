use color_eyre::Result;

pub mod dd;

pub trait StatsdClient: 'static + Send + Sync {
    fn count(&self, stat: &str, count: i64, tags: Vec<String>) -> Result<()>;

    fn incr_by_value(&self, stat: &str, value: i64, tags: Vec<String>) -> Result<()>;

    fn gauge(&self, stat: &str, val: &str, tags: Vec<String>) -> Result<()>;

    fn distribution(&self, stat: &str, val: &str, tags: Vec<String>) -> Result<()>;
}
