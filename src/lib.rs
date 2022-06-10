#![feature(pin_macro)]
#![feature(generators, generator_trait)]

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
#[macro_export]
macro_rules! async_generator {
    ($state:ident, $future:expr) => {
        ::core::pin::pin!($crate::Generator::new(&($state), $future))
    };
}

#[pin_project::pin_project]
pub struct AsyncGenerator<G> {
    #[pin]
    generator: G,
}

impl<G: core::ops::Generator<core::task::Waker, Yield = core::task::Poll<Y>, Return = ()>, Y>
    AsyncGenerator<G>
{
    pub fn new(generator: G) -> Self {
        Self { generator }
    }
}

impl<G: core::ops::Generator<core::task::Waker, Yield = core::task::Poll<Y>, Return = ()>, Y>
    futures_util::stream::Stream for AsyncGenerator<G>
{
    type Item = Y;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        match self.project().generator.resume(cx.waker().clone()) {
            core::ops::GeneratorState::Yielded(val) => val.map(Some),
            core::ops::GeneratorState::Complete(()) => core::task::Poll::Ready(None),
        }
    }
}

#[macro_export]
macro_rules! genyield {
    ($waker:ident, $value:expr) => {{
        let _ = &$waker;
        $waker = yield ::core::task::Poll::Ready($value);
        let _ = &$waker;
    }};
}

#[macro_export]
macro_rules! genawait {
    ($waker:ident, $future:expr) => {{
        use ::core::future::Future as _;

        let mut fut = $future;
        loop {
            // SAFETY: The generator is part of the stream future which is !Unpin.
            //         Thus, we can only get here if the future was pinned.
            let fut = unsafe { core::pin::Pin::new_unchecked(&mut fut) };

            match fut.poll(&mut ::core::task::Context::from_waker(&$waker)) {
                core::task::Poll::Ready(val) => break val,
                core::task::Poll::Pending => {
                    let _ = &$waker;
                    $waker = yield core::task::Poll::Pending;
                    let _ = &$waker;
                }
            }
        }
    }};
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
    async fn variant_1() {
        let genstate = super::GeneratorState::default();
        let mut gen = async_generator!(genstate, |genstate| doit(genstate));

        while let Some(val) = gen.next().await {
            eprintln!("VAL: {:#?}", val);
        }
    }

    fn makegen() -> impl futures_util::stream::Stream<Item = usize> {
        super::AsyncGenerator::new(|mut waker| {
            genyield!(waker, 42usize);
            genawait!(
                waker,
                tokio::time::sleep(tokio::time::Duration::from_millis(2000))
            );
            genyield!(waker, 43usize);
        })
    }

    #[tokio::test]
    async fn variant_2a() {
        let mut generator = makegen();
        while let Some(val) = generator.next().await {
            eprintln!("VAL: {:#?}", val);
        }
    }

    #[tokio::test]
    async fn variant_2b() {
        let mut generator = super::AsyncGenerator::new(|mut waker| {
            genyield!(waker, 42usize);
            eprintln!("yoooo1");
            genyield!(waker, 43usize);
            eprintln!("yoooo2");
        });
        while let Some(val) = generator.next().await {
            eprintln!("VAL: {:#?}", val);
        }
    }
}
