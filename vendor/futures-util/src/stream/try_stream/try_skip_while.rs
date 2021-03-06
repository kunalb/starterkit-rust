use core::fmt;
use core::pin::Pin;
use futures_core::future::TryFuture;
use futures_core::stream::{Stream, TryStream, FusedStream};
use futures_core::task::{Context, Poll};
#[cfg(feature = "sink")]
use futures_sink::Sink;
use pin_project::{pin_project, project};

/// Stream for the [`try_skip_while`](super::TryStreamExt::try_skip_while)
/// method.
#[pin_project]
#[must_use = "streams do nothing unless polled"]
pub struct TrySkipWhile<St, Fut, F> where St: TryStream {
    #[pin]
    stream: St,
    f: F,
    #[pin]
    pending_fut: Option<Fut>,
    pending_item: Option<St::Ok>,
    done_skipping: bool,
}

impl<St, Fut, F> fmt::Debug for TrySkipWhile<St, Fut, F>
where
    St: TryStream + fmt::Debug,
    St::Ok: fmt::Debug,
    Fut: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrySkipWhile")
            .field("stream", &self.stream)
            .field("pending_fut", &self.pending_fut)
            .field("pending_item", &self.pending_item)
            .field("done_skipping", &self.done_skipping)
            .finish()
    }
}

impl<St, Fut, F> TrySkipWhile<St, Fut, F>
    where St: TryStream,
          F: FnMut(&St::Ok) -> Fut,
          Fut: TryFuture<Ok = bool, Error = St::Error>,
{
    pub(super) fn new(stream: St, f: F) -> TrySkipWhile<St, Fut, F> {
        TrySkipWhile {
            stream,
            f,
            pending_fut: None,
            pending_item: None,
            done_skipping: false,
        }
    }

    delegate_access_inner!(stream, St, ());
}

impl<St, Fut, F> Stream for TrySkipWhile<St, Fut, F>
    where St: TryStream,
          F: FnMut(&St::Ok) -> Fut,
          Fut: TryFuture<Ok = bool, Error = St::Error>,
{
    type Item = Result<St::Ok, St::Error>;

    #[project]
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        #[project]
        let TrySkipWhile { mut stream, f, mut pending_fut, pending_item, done_skipping } = self.project();

        if *done_skipping {
            return stream.try_poll_next(cx);
        }

        Poll::Ready(loop {
            if let Some(fut) = pending_fut.as_mut().as_pin_mut() {
                let skipped = ready!(fut.try_poll(cx)?);
                let item = pending_item.take();
                pending_fut.set(None);
                if !skipped {
                    *done_skipping = true;
                    break item.map(Ok);
                }
            } else if let Some(item) = ready!(stream.as_mut().try_poll_next(cx)?) {
                pending_fut.set(Some(f(&item)));
                *pending_item = Some(item);
            } else {
                break None;
            }
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let pending_len = if self.pending_item.is_some() { 1 } else { 0 };
        let (_, upper) = self.stream.size_hint();
        let upper = match upper {
            Some(x) => x.checked_add(pending_len),
            None => None,
        };
        (0, upper) // can't know a lower bound, due to the predicate
    }
}

impl<St, Fut, F> FusedStream for TrySkipWhile<St, Fut, F>
    where St: TryStream + FusedStream,
          F: FnMut(&St::Ok) -> Fut,
          Fut: TryFuture<Ok = bool, Error = St::Error>,
{
    fn is_terminated(&self) -> bool {
        self.pending_item.is_none() && self.stream.is_terminated()
    }
}

// Forwarding impl of Sink from the underlying stream
#[cfg(feature = "sink")]
impl<S, Fut, F, Item, E> Sink<Item> for TrySkipWhile<S, Fut, F>
    where S: TryStream + Sink<Item, Error = E>,
{
    type Error = E;

    delegate_sink!(stream, Item);
}
