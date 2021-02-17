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

pub trait FrameDecodedCallback<P: HandlesProvider>:
    FnMut(DQBuffer<Capture, P::HandleType>) + Send + 'static
{
}
impl<P, F> FrameDecodedCallback<P> for F
where
    P: HandlesProvider,
    F: FnMut(DQBuffer<Capture, P::HandleType>) + Send + 'static,
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
