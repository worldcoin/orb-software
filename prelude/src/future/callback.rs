use std::{future::Future, pin::Pin, sync::Arc};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

#[derive(Clone)]
pub struct Callback<Args = (), Ret = ()>(
    Arc<dyn Fn(Args) -> BoxFuture<Ret> + Send + Sync + 'static>,
);

impl Callback {
    pub fn new<F, Fut, Args, Ret>(f: F) -> Callback<Args, Ret>
    where
        F: Fn(Args) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Ret> + Send + 'static,
        Args: Send + 'static,
        Ret: 'static,
    {
        let f = Arc::new(f);
        Callback(Arc::new(move |args: Args| {
            let f = f.clone();
            Box::pin(f(args))
        }))
    }
}

impl<Args, Ret> Callback<Args, Ret>
where
    Args: Send + 'static,
    Ret: 'static,
{
    pub fn call(&self, args: Args) -> BoxFuture<Ret> {
        (self.0)(args)
    }
}

#[cfg(test)]
mod tests {
    use super::Callback;

    #[tokio::test]
    async fn test() {
        let cb = Callback::new(async |(x, y): (i32, i32)| x + y);
        let result = cb.call((3, 4)).await;
        assert_eq!(result, 7);
    }
}
