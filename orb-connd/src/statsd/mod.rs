use color_eyre::Result;

pub mod dd;

pub trait StatsdClient: 'static + Send + Sync {
    fn count<S: AsRef<str> + Sync + Send>(
        &self,
        stat: &str,
        count: i64,
        tags: &[S],
    ) -> impl Future<Output = Result<()>> + Send + Sync;

    fn incr_by_value<S: AsRef<str> + Sync + Send>(
        &self,
        stat: &str,
        value: i64,
        tags: &[S],
    ) -> impl Future<Output = Result<()>> + Send + Sync;

    fn gauge<S: AsRef<str> + Sync + Send>(
        &self,
        stat: &str,
        val: &str,
        tags: &[S],
    ) -> impl Future<Output = Result<()>> + Send + Sync;
}
