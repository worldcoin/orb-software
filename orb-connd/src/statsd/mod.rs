use async_trait::async_trait;
use color_eyre::Result;

pub mod dd;

#[async_trait]
pub trait StatsdClient: 'static + Send + Sync {
    async fn count(&self, stat: &str, count: i64, tags: Vec<String>) -> Result<()>;

    async fn incr_by_value(
        &self,
        stat: &str,
        value: i64,
        tags: Vec<String>,
    ) -> Result<()>;

    async fn gauge(&self, stat: &str, val: &str, tags: Vec<String>) -> Result<()>;
}
