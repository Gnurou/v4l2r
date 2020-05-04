//! Abstracts the different kinds of backing memory (MMAP, USERPTR, DMABUF)
//! supported by V4L2.
mod dmabuf;
mod mmap;
mod userptr;

pub use dmabuf::*;
pub use mmap::*;
pub use userptr::*;
