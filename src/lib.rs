#![feature(pin_macro)]

/// A future which returns [::core::task::Poll::Pending] once
///
/// The intention is to make the generators poll function return a yielded
/// value.
/// This should only be used with generators since the waker is neither stored
/// nor used.
pub struct YieldFuture {
    polled: bool,
}

impl core::future::Future for YieldFuture {
    type Output = ();

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        if self.polled {
            core::task::Poll::Ready(())
        } else {
            self.polled = true;
            core::task::Poll::Pending
        }
    }
}

/// Shared state between the future and the stream of a generator
#[derive(Default)]
pub struct GeneratorState<T> {
    inner: core::cell::Cell<Option<T>>,
}

impl<T> GeneratorState<T> {
    pub fn doyield(&self, value: T) -> YieldFuture {
        self.inner.set(Some(value));
        YieldFuture { polled: false }
    }
}

/// An async generator
#[pin_project::pin_project]
pub struct Generator<'a, S, F> {
    state: &'a GeneratorState<S>,

    #[pin]
    future: F,
}

impl<'a, S, F: core::future::Future<Output = ()>> Generator<'a, S, F> {
    pub fn new<MakeFut: Fn(&'a GeneratorState<S>) -> F>(
        state: &'a GeneratorState<S>,
        make_future: MakeFut,
    ) -> Self {
        Self {
            state,
            future: make_future(state),
        }
    }
}

impl<'a, S, F: core::future::Future<Output = ()>> futures_util::stream::Stream
    for Generator<'a, S, F>
{
    type Item = S;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        let me = self.project();

        match me.future.poll(cx) {
            core::task::Poll::Pending => {
                if let Some(val) = me.state.inner.replace(None) {
                    core::task::Poll::Ready(Some(val))
                } else {
                    core::task::Poll::Pending
                }
            }
            core::task::Poll::Ready(_) => core::task::Poll::Ready(None),
        }
    }
}

/// convenience macro for creating and pinning a generator
macro_rules! async_generator {
    ($state:ident, $future:expr) => {
        ::core::pin::pin!($crate::Generator::new(&($state), $future))
    };
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt as _;

    async fn doit(genstate: &super::GeneratorState<u32>) {
        genstate.doyield(42).await;

        eprintln!("yoooo1");

        genstate.doyield(43).await;

        eprintln!("yoooo2");
    }

    #[tokio::test]
    async fn it_works() {
        let genstate = super::GeneratorState::default();
        let mut gen = async_generator!(genstate, |genstate| doit(genstate));

        while let Some(val) = gen.next().await {
            eprintln!("VAL: {:#?}", val);
        }
    }
}
