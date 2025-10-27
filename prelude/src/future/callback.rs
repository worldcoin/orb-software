use std::{future::Future, pin::Pin, sync::Arc};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

#[derive(Clone)]
pub struct Callback<Args = (), Ret = ()>(
    Arc<dyn Fn(Args) -> BoxFuture<Ret> + Send + Sync + 'static>,
);

pub trait IntoCallback<Args, Ret> {
    fn into_callback(self) -> Callback<Args, Ret>;
}

impl<Args, Ret> Callback<Args, Ret> {
    pub fn new<T>(t: T) -> Self
    where
        T: IntoCallback<Args, Ret>,
    {
        t.into_callback()
    }

    pub fn call(&self, args: Args) -> BoxFuture<Ret> {
        (self.0)(args)
    }
}

impl<Args, Ret, F, Fut> IntoCallback<Args, Ret> for F
where
    F: Fn(Args) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Ret> + Send + 'static,
    Args: Send + 'static,
    Ret: 'static,
{
    fn into_callback(self) -> Callback<Args, Ret> {
        let f = Arc::new(self);
        Callback(Arc::new(move |args: Args| {
            let f = f.clone();
            Box::pin(f(args))
        }))
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
