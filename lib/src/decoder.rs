use crate::{
    device::queue::{
        direction::{Capture, Output},
        dqbuf::DQBuffer,
        handles_provider::HandlesProvider,
        CanceledBuffer, FormatBuilder,
    },
    memory::BufferHandles,
    Rect,
};

pub mod format;
pub mod stateful;

pub enum CompletedInputBuffer<OP: BufferHandles> {
    Dequeued(DQBuffer<Output, OP>),
    Canceled(CanceledBuffer<OP>),
}

pub trait InputDoneCallback<OP: BufferHandles>: Fn(CompletedInputBuffer<OP>) {}
impl<OP, F> InputDoneCallback<OP> for F
where
    OP: BufferHandles,
    F: Fn(CompletedInputBuffer<OP>),
{
}

/// TODO: add errors?
pub enum DecoderEvent<P: HandlesProvider> {
    /// Emitted when a frame is decoded.
    ///
    /// The parameter is the dequeued buffer, containing the plane handles of
    /// the decoded frame as well as its V4L2 parameters such as flags. The
    /// flags remain untouched, but the client should not take action on some
    /// of them: for instance, when the `V4L2_BUF_FLAG_LAST` is set, the proper
    /// corresponding event (resolution change or end of stream) will be
    /// signaled appropriately.
    FrameDecoded(DQBuffer<Capture, P::HandleType>),
    /// Emitted when a previously requested `drain` request completes.
    ///
    /// When this event is emitted, the client knows that all the frames
    /// corresponding to all the input buffers queued before the `drain` request
    /// have been emitted.
    EndOfStream,
}

pub trait DecoderEventCallback<P: HandlesProvider>:
    FnMut(DecoderEvent<P>) + Send + 'static
{
}
impl<P, F> DecoderEventCallback<P> for F
where
    P: HandlesProvider,
    F: FnMut(DecoderEvent<P>) + Send + 'static,
{
}

pub struct FormatChangedReply<P: HandlesProvider> {
    pub provider: P,
    pub mem_type: <P::HandleType as BufferHandles>::SupportedMemoryType,
    pub num_buffers: usize,
}

pub trait FormatChangedCallback<P: HandlesProvider>:
    Fn(FormatBuilder, Rect, usize) -> anyhow::Result<FormatChangedReply<P>> + Send + 'static
{
}
impl<P, F> FormatChangedCallback<P> for F
where
    P: HandlesProvider,
    F: Fn(FormatBuilder, Rect, usize) -> anyhow::Result<FormatChangedReply<P>> + Send + 'static,
{
}
