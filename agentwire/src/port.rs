#![allow(clippy::borrow_as_ptr)]
//! Bi-directional channel for a computation unit.
//!
//! There are two kinds of ports: internal and shared.
//!
//! # Internal ports
//!
//! Internal port is used when the agent is located in the same process as the
//! broker (task-based and thread-based agents).
//!
//! ```ignore
//! use agentwire::{Agent, Port};
//!
//! struct Foo;
//!
//! impl Agent for Foo {
//!     const NAME: &'static str = "foo";
//! }
//!
//! impl Port for Foo {
//!     type Input = Input;
//!     type Output = Output;
//!
//!     // Set to `0` to not buffer the input data.
//!     const INPUT_CAPACITY: usize = 0;
//!     // Set to `0` to not buffer the output data.
//!     const OUTPUT_CAPACITY: usize = 0;
//! }
//!
//! enum Input {
//!     // ..
//! }
//!
//! enum Output {
//!     // ..
//! }
//! ```
//!
//! # Shared ports
//!
//! Shared port is used when the agent is located in a separate process. The
//! shared memory is used to transfer the data between the processes.
//!
//! A shared port must define the buffer sizes for the initial state, input
//! messages, and output messages. The following example sets the sizes for
//! simple types. If a type contains dynamic data, e.g. vectors or strings, then
//! the buffer size should be set to the maximum possible size of the data.
//!
//! ```ignore
//! use agentwire::{Agent, Port, SharedPort};
//! use rkyv::{Archive, Deserialize, Serialize};
//!
//! #[derive(Archive, Serialize, Deserialize)]
//! struct Foo {
//!     // ..
//! }
//!
//! impl Agent for Foo {
//!     const NAME: &'static str = "foo";
//! }
//!
//! impl Port for Foo {
//!     type Input = Input;
//!     type Output = Output;
//!
//!     // Set to `0` to not buffer the input data.
//!     const INPUT_CAPACITY: usize = 0;
//!     // Set to `0` to not buffer the output data.
//!     const OUTPUT_CAPACITY: usize = 0;
//! }
//!
//! impl SharedPort for Foo {
//!     const SERIALIZED_INIT_SIZE: usize =
//!         size_of::<usize>() + size_of::<<Foo as Archive>::Archived>();
//!     const SERIALIZED_INPUT_SIZE: usize =
//!         size_of::<usize>() + size_of::<<Input as Archive>::Archived>();
//!     const SERIALIZED_OUTPUT_SIZE: usize =
//!         size_of::<usize>() + size_of::<<Output as Archive>::Archived>();
//! }
//!
//! #[derive(Archive, Serialize, Deserialize)]
//! enum Input {
//!     // ..
//! }
//!
//! #[derive(Archive, Serialize, Deserialize)]
//! enum Output {
//!     // ..
//! }
//! ```

use futures::{
    channel::{
        mpsc::{self, SendError},
        oneshot,
    },
    future::{select, Either},
    prelude::*,
    select_biased,
    stream::FusedStream,
};
use libc::{c_int, c_uint, sem_t};
use nix::{
    errno::Errno,
    sys::{
        memfd::{memfd_create, MemFdCreateFlag},
        mman::{mmap, munmap, MapFlags, ProtFlags},
    },
    unistd::ftruncate,
};
use rkyv::{
    de::deserializers::SharedDeserializeMap,
    ser::{
        serializers::{
            AllocScratch, BufferSerializer, CompositeSerializer, FallbackScratch,
            HeapScratch, SharedSerializeMap,
        },
        Serializer,
    },
    Archive, Deserialize, Infallible, Serialize,
};
use std::{
    cmp::max,
    ffi::{CString, NulError},
    fmt::Debug,
    io,
    marker::PhantomData,
    mem,
    num::NonZeroUsize,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    pin::Pin,
    ptr, slice,
    task::{Context, Poll},
    time::Instant,
};
use thiserror::Error;
use tokio::task;

const SCRATCH_SIZE: usize = 1024;

/// Error occured during shared memory creation.
#[derive(Error, Debug)]
pub enum CreateSharedMemoryError {
    /// Invalid shared memory name.
    #[error("invalid name: {0}")]
    InvalidName(NulError),
    /// Error occured during `memfd_create`.
    #[error("memfd_create: {0}")]
    MemfdCreate(Errno),
    /// Error occured during `ftruncate`.
    #[error("ftruncate: {0}")]
    Ftruncate(Errno),
    #[error("mmap: {0}")]
    /// Error occured during `mmap`.
    Mmap(Errno),
    /// Error occured during semaphore initialization.
    #[error("sem_init: {0}")]
    SemInit(io::Error),
}

/// Error occured during shared memory destruction.
#[derive(Error, Debug)]
pub enum DestroySharedMemoryError {
    /// Error occured during `munmap`.
    #[error("munmap: {0}")]
    Munmap(Errno),
    /// Error occured during semaphore destruction.
    #[error("sem_destroy: {0}")]
    SemDestroy(io::Error),
}

/// Error returned by [`Outer::send_unjam`].
#[derive(Error, Debug)]
pub enum SendUnjamError {
    /// Error occured during message sending.
    #[error("send: {0}")]
    Send(#[from] SendError),
    /// Port is closed.
    #[error("port is closed")]
    Closed,
}

/// Bi-directional channel description.
pub trait Port: 'static {
    /// Input channel message type.
    ///
    /// Set to `!` if the agent doesn't have input, e.g. a raw sensor.
    type Input: Send + Debug;

    /// Output channel message type.
    ///
    /// Set to `!` if the agent doesn't have output, e.g. a raw actuator.
    type Output: Send + Debug;

    /// Input channel capacity.
    ///
    /// Set to `0` if the input data should to be as fresh as possible.
    const INPUT_CAPACITY: usize;

    /// Output channel capacity.
    ///
    /// Set to `0` if the output data should to be as fresh as possible.
    const OUTPUT_CAPACITY: usize;
}

/// Shared memory serializer.
pub type SharedSerializer<'a> = CompositeSerializer<
    BufferSerializer<&'a mut [u8]>,
    FallbackScratch<HeapScratch<SCRATCH_SIZE>, AllocScratch>,
    SharedSerializeMap,
>;

/// Bi-directional channel description in shared memory.
#[allow(clippy::module_name_repetitions)]
pub trait SharedPort: Port
where
    Self::Input: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    Self::Output: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <Self::Output as Archive>::Archived:
        Deserialize<Self::Output, SharedDeserializeMap>,
{
    /// Buffer size for input messages. Must be at least `size_of::<usize>()`
    /// for a zero-sized input.
    const SERIALIZED_INPUT_SIZE: usize;

    /// Buffer size for output messages. Must be at least `size_of::<usize>()`
    /// for a zero-sized output.
    const SERIALIZED_OUTPUT_SIZE: usize;

    /// Buffer size for initial agent state. Must be at least
    /// `size_of::<usize>()` for a zero-sized state.
    const SERIALIZED_INIT_SIZE: usize;
}

/// Input message.
#[derive(Debug)]
pub struct Input<T: Port> {
    /// Input value.
    pub value: T::Input,
    /// Source data timestamp.
    pub source_ts: Instant,
}

/// Archived input message.
pub struct ArchivedInput<'a, T: Port>
where
    T::Input: Archive,
{
    /// Archived input value.
    pub value: &'a <T::Input as Archive>::Archived,
    /// Source data timestamp.
    pub source_ts: Instant,
}

/// Output message.
#[derive(Debug)]
pub struct Output<T: Port> {
    /// Output value.
    pub value: T::Output,
    /// Source data timestamp.
    pub source_ts: Instant,
}

/// A handle for bi-directional communication for the outside of the computation
/// unit. The type implements both [`Sink`] and [`Stream`] for the input and the
/// output channels respectively.
pub struct Outer<T: Port> {
    /// Sender channel for the computation unit input.
    pub tx: OuterTx<T>,
    /// Receiver channel for the computation unit output.
    pub rx: OuterRx<T>,
}

/// A handle for bi-directional communication for the inside of the computation
/// unit. The type implements both [`Sink`] and [`Stream`] for the input and the
/// output channels respectively.
pub struct Inner<T: Port> {
    /// Sender channel for the computation unit output.
    pub tx: InnerTx<T>,
    /// Receiver channel for the computation unit input.
    pub rx: InnerRx<T>,
}

/// A handle for bi-directional communication for the inside of the computation
/// unit, which is located in another process.
pub struct RemoteInner<T>
where
    T: SharedPort + Debug + Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T as Archive>::Archived: Deserialize<T, Infallible>,
    T::Input: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    T::Output: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T::Output as Archive>::Archived: Deserialize<T::Output, SharedDeserializeMap>,
{
    shared_memory: *mut SharedMemory<T>,
    scratch: Option<FallbackScratch<HeapScratch<SCRATCH_SIZE>, AllocScratch>>,
}

/// Sender channel for the computation unit input.
pub type OuterTx<T> = mpsc::Sender<Input<T>>;

/// Receiver channel for the computation unit output.
pub type OuterRx<T> = mpsc::Receiver<Output<T>>;

/// Sender channel for the computation unit output.
pub type InnerTx<T> = mpsc::Sender<Output<T>>;

/// Receiver channel for the computation unit input.
pub type InnerRx<T> = mpsc::Receiver<Input<T>>;

type InitialInputs = Vec<(Box<[u8]>, Instant)>;

/// Creates a new bi-directional channel.
#[must_use]
pub fn new<T: Port>() -> (Inner<T>, Outer<T>) {
    let (input_tx, input_rx) = mpsc::channel(T::INPUT_CAPACITY);
    let (output_tx, output_rx) = mpsc::channel(T::OUTPUT_CAPACITY);
    let inner = Inner {
        tx: output_tx,
        rx: input_rx,
    };
    let outer = Outer {
        tx: input_tx,
        rx: output_rx,
    };
    (inner, outer)
}

impl<T: Port> Input<T> {
    /// Creates a new input value with the source timestamp of now.
    pub fn new(value: T::Input) -> Self {
        Self {
            value,
            source_ts: Instant::now(),
        }
    }

    /// Creates a new input value with the source timestamp of the original
    /// input.
    pub fn derive<O: Port>(&self, value: O::Input) -> Input<O> {
        Input {
            value,
            source_ts: self.source_ts,
        }
    }

    /// Creates a new output value with the source timestamp of the input.
    pub fn chain(&self, value: T::Output) -> Output<T> {
        Output {
            value,
            source_ts: self.source_ts,
        }
    }

    /// Returns a closure, which creates a new output value with the source
    /// timestamp of the input.
    pub fn chain_fn(&self) -> impl Fn(T::Output) -> Output<T> + use<T> {
        let source_ts = self.source_ts;
        move |value| Output { value, source_ts }
    }
}

impl<T: Port> ArchivedInput<'_, T>
where
    T::Input: Archive,
{
    /// Creates a new output value with the source timestamp of the input.
    pub fn chain(&self, value: T::Output) -> Output<T> {
        Output {
            value,
            source_ts: self.source_ts,
        }
    }

    /// Returns a closure, which creates a new output value with the source
    /// timestamp of the input.
    pub fn chain_fn(&self) -> impl Fn(T::Output) -> Output<T> + use<T> {
        let source_ts = self.source_ts;
        move |value| Output { value, source_ts }
    }
}

impl<T: Port> Output<T> {
    /// Creates a new output value with the source timestamp of now.
    pub fn new(value: T::Output) -> Self {
        Self {
            value,
            source_ts: Instant::now(),
        }
    }

    /// Creates a new output value with the source timestamp of the original
    /// output.
    pub fn derive<O: Port>(&self, value: O::Output) -> Output<O> {
        Output {
            value,
            source_ts: self.source_ts,
        }
    }

    /// Returns a closure, which creates a new output value with the source
    /// timestamp of the original output.
    pub fn derive_fn<O: Port>(&self) -> impl Fn(O::Output) -> Output<O> + use<T, O> {
        let source_ts = self.source_ts;
        move |value| Output { value, source_ts }
    }

    /// Creates a new input value with the source timestamp of the output.
    pub fn chain<O: Port>(&self, value: O::Input) -> Input<O> {
        Input {
            value,
            source_ts: self.source_ts,
        }
    }

    /// Returns a closure, which creates a new input value with the source
    /// timestamp of the output.
    pub fn chain_fn<O: Port>(&self) -> impl Fn(O::Input) -> Input<O> + use<T, O> {
        let source_ts = self.source_ts;
        move |value| Input { value, source_ts }
    }
}

impl<T: Port> Outer<T> {
    /// Sends a message avoiding jams. Reading a message from the queue if
    /// necessary.
    ///
    /// This is for situations where the agent may be blocked by sending a
    /// message to the broker, but the broker is not listening to new messages
    /// from the agent. Instead the broker sends a message to the agent and
    /// blocks until it's received by the agent.
    #[allow(clippy::mut_mut)] // triggered by `select!` internals
    pub async fn send_unjam(
        &mut self,
        message: Input<T>,
    ) -> Result<(), SendUnjamError> {
        let mut send = self.tx.send(message).fuse();
        let mut recv = self.rx.next();
        loop {
            select_biased! {
                result = send => break Ok(result?),
                item = recv => match item {
                    Some(item) => drop(item),
                    None => break Err(SendUnjamError::Closed),
                }
            }
        }
    }
}

impl<T: Port> Stream for Outer<T> {
    type Item = Output<T>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.rx).poll_next(cx)
    }
}

impl<T: Port> FusedStream for Outer<T> {
    fn is_terminated(&self) -> bool {
        self.rx.is_terminated()
    }
}

impl<T: Port> Sink<Input<T>> for Outer<T> {
    type Error = SendError;

    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.tx).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Input<T>) -> Result<(), Self::Error> {
        Pin::new(&mut self.tx).start_send(item)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.tx).poll_flush(cx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.tx).poll_close(cx)
    }
}

impl<T: Port> Stream for Inner<T> {
    type Item = Input<T>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.rx).poll_next(cx)
    }
}

impl<T: Port> FusedStream for Inner<T> {
    fn is_terminated(&self) -> bool {
        self.rx.is_terminated()
    }
}

impl<T: Port> Sink<Output<T>> for Inner<T> {
    type Error = SendError;

    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.tx).poll_ready(cx)
    }

    fn start_send(
        mut self: Pin<&mut Self>,
        item: Output<T>,
    ) -> Result<(), Self::Error> {
        Pin::new(&mut self.tx).start_send(item)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.tx).poll_flush(cx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.tx).poll_close(cx)
    }
}

// This is a header of a shared memory. Right after the header, there is a raw
// data buffer. On initialization it contains the initial agent state. After
// initialization it contains the following content in the specific order:
//
// 1. Input buffer 0
// 2. Input buffer 1
// 3. Output buffer
struct SharedMemory<T>
where
    T: SharedPort + Debug + Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T as Archive>::Archived: Deserialize<T, Infallible>,
    T::Input: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    T::Output: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T::Output as Archive>::Archived: Deserialize<T::Output, SharedDeserializeMap>,
{
    input_ts: [Instant; 2],
    input_tx: sem_t,
    input_rx: sem_t,
    input_count: usize,
    input_index: usize,
    output_ts: Instant,
    output_tx: sem_t,
    output_rx: sem_t,
    _marker: PhantomData<T>,
}

impl<T> SharedMemory<T>
where
    T: SharedPort + Debug + Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T as Archive>::Archived: Deserialize<T, Infallible>,
    T::Input: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    T::Output: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T::Output as Archive>::Archived: Deserialize<T::Output, SharedDeserializeMap>,
{
    fn size_of() -> NonZeroUsize {
        let size = mem::size_of::<Self>()
            + max(
                mem::size_of::<usize>() + mem::size_of::<T::Archived>(),
                T::SERIALIZED_INPUT_SIZE * 2 + T::SERIALIZED_OUTPUT_SIZE,
            );
        NonZeroUsize::new(size).expect("to always be positive")
    }

    unsafe fn create(
        name: &str,
    ) -> Result<(*mut Self, OwnedFd), CreateSharedMemoryError> {
        let size = Self::size_of();
        let name = CString::new(name).map_err(CreateSharedMemoryError::InvalidName)?;
        let raw_fd = memfd_create(&name, MemFdCreateFlag::empty())
            .map_err(CreateSharedMemoryError::MemfdCreate)?
            as RawFd;
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        let len = size
            .get()
            .try_into()
            .expect("shared memory size is extremely large");
        ftruncate(fd.as_raw_fd(), len).map_err(CreateSharedMemoryError::Ftruncate)?;
        let ptr = unsafe {
            mmap(
                None,
                size,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
            .map_err(CreateSharedMemoryError::Mmap)?
            .cast::<Self>()
        };
        unsafe {
            sem_init(&mut (*ptr).input_tx, 1, 0)
                .map_err(CreateSharedMemoryError::SemInit)?;
            sem_init(&mut (*ptr).input_rx, 1, 0)
                .map_err(CreateSharedMemoryError::SemInit)?;
            sem_init(&mut (*ptr).output_tx, 1, 1)
                .map_err(CreateSharedMemoryError::SemInit)?;
            sem_init(&mut (*ptr).output_rx, 1, 0)
                .map_err(CreateSharedMemoryError::SemInit)?;
            (*ptr).input_count = 0;
            (*ptr).input_index = 0;
        }
        Ok((ptr, fd))
    }

    unsafe fn from_fd(fd: OwnedFd) -> Result<*mut Self, Errno> {
        let ptr = unsafe {
            mmap(
                None,
                Self::size_of(),
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )?
            .cast::<Self>()
        };
        drop(fd);
        Ok(ptr)
    }

    unsafe fn destroy(ptr: *mut Self) -> Result<(), DestroySharedMemoryError> {
        unsafe {
            sem_destroy(&mut (*ptr).input_tx)
                .map_err(DestroySharedMemoryError::SemDestroy)?;
            sem_destroy(&mut (*ptr).input_rx)
                .map_err(DestroySharedMemoryError::SemDestroy)?;
            sem_destroy(&mut (*ptr).output_tx)
                .map_err(DestroySharedMemoryError::SemDestroy)?;
            sem_destroy(&mut (*ptr).output_rx)
                .map_err(DestroySharedMemoryError::SemDestroy)?;
            munmap(ptr.cast(), Self::size_of().get())
                .map_err(DestroySharedMemoryError::Munmap)?;
        }
        Ok(())
    }

    unsafe fn init_state(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                ptr::addr_of_mut!(*self).add(1).cast::<u8>(),
                T::SERIALIZED_INIT_SIZE,
            )
        }
    }

    unsafe fn input(&mut self, n: usize) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                ptr::addr_of_mut!(*self)
                    .add(1)
                    .cast::<u8>()
                    .add(T::SERIALIZED_INPUT_SIZE * n),
                T::SERIALIZED_INPUT_SIZE,
            )
        }
    }

    unsafe fn output(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                ptr::addr_of_mut!(*self)
                    .add(1)
                    .cast::<u8>()
                    .add(T::SERIALIZED_INPUT_SIZE * 2),
                T::SERIALIZED_OUTPUT_SIZE,
            )
        }
    }
}

impl<T> Inner<T>
where
    T: SharedPort + Debug + Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T as Archive>::Archived: Deserialize<T, Infallible>,
    T::Input: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    T::Output: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T::Output as Archive>::Archived: Deserialize<T::Output, SharedDeserializeMap>,
{
    /// Sets up shared memory for this channel.
    #[expect(clippy::type_complexity)]
    pub fn into_shared_memory(
        self,
        name: &str,
        init_state: &T,
        initial_inputs: InitialInputs,
    ) -> Result<
        (
            OwnedFd,
            impl Future<Output = Result<(Self, InitialInputs), DestroySharedMemoryError>>,
        ),
        CreateSharedMemoryError,
    > {
        let Self { tx, rx } = self;
        let (ptr, fd) = unsafe { SharedMemory::<T>::create(name)? };
        let addr = ptr as usize;
        let (stop_tx_tx, stop_tx_rx) = oneshot::channel();
        let (stop_rx_tx, stop_rx_rx) = oneshot::channel();
        set_init_state(addr, init_state);
        let tx_task = spawn_shared_tx_task(tx, addr, stop_tx_rx);
        let rx_task = spawn_shared_rx_task(rx, addr, stop_rx_rx, initial_inputs);
        let close = async move {
            let _ = stop_tx_tx.send(());
            let _ = stop_rx_tx.send(());
            let tx = tx_task.await.unwrap();
            let (rx, mut inputs) = rx_task.await.unwrap();
            unsafe {
                let shared_memory = addr as *mut SharedMemory<T>;
                assert!((*shared_memory).input_count <= 2);
                for mut i in 0..(*shared_memory).input_count {
                    if (*shared_memory).input_count == 2
                        && (*shared_memory).input_index == 0
                    {
                        i = (i + 1) % 2;
                    }
                    let input = Box::from(&*(*shared_memory).input(i));
                    let input_ts = (*shared_memory).input_ts[i];
                    inputs.push((input, input_ts));
                }
                SharedMemory::destroy(shared_memory)?;
                Ok((Self { tx, rx }, inputs))
            }
        };
        Ok((fd, close))
    }
}

impl<T> RemoteInner<T>
where
    T: SharedPort + Debug + Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T as Archive>::Archived: Deserialize<T, Infallible>,
    T::Input: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    T::Output: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T::Output as Archive>::Archived: Deserialize<T::Output, SharedDeserializeMap>,
{
    /// Creates a channel from the shared memory.
    pub fn from_shared_memory(shmem_fd: OwnedFd) -> Result<Self, Errno> {
        Ok(RemoteInner {
            shared_memory: unsafe { SharedMemory::<T>::from_fd(shmem_fd)? },
            scratch: Some(FallbackScratch::default()),
        })
    }

    /// Reads the initial state.
    #[allow(clippy::missing_panics_doc)]
    pub fn init_state(&mut self) -> &<T as Archive>::Archived {
        unsafe {
            let init_state =
                deserialize_message::<T>((*self.shared_memory).init_state());
            sem_post(&mut (*self.shared_memory).input_tx).expect("semaphore failure");
            init_state
        }
    }

    /// Waits for a value on the receiver half.
    #[allow(clippy::missing_panics_doc)]
    pub fn recv(&mut self) -> ArchivedInput<'_, T> {
        unsafe {
            sem_wait(&mut (*self.shared_memory).input_rx).expect("semaphore failure");
            let input_index = 1 - (*self.shared_memory).input_index;
            let value = deserialize_message::<T::Input>(
                (*self.shared_memory).input(input_index),
            );
            let source_ts = (*self.shared_memory).input_ts[input_index];
            sem_post(&mut (*self.shared_memory).input_tx).expect("semaphore failure");
            ArchivedInput { value, source_ts }
        }
    }

    /// Tries to receive a value on the receiver half. This function doesn't
    /// block and returns `None` if the channel is empty.
    #[allow(clippy::missing_panics_doc)]
    pub fn try_recv(&mut self) -> Option<ArchivedInput<'_, T>> {
        unsafe {
            if sem_getvalue(&mut (*self.shared_memory).input_rx)
                .expect("semaphore failure")
                > 0
            {
                Some(self.recv())
            } else {
                None
            }
        }
    }

    /// Sends a value on this channel.
    #[allow(clippy::missing_panics_doc)]
    pub fn send(&mut self, output: &Output<T>) {
        unsafe {
            sem_wait(&mut (*self.shared_memory).output_tx).expect("semaphore failure");
            serialize_message(
                (*self.shared_memory).output(),
                &mut self.scratch,
                &output.value,
            );
            (*self.shared_memory).output_ts = output.source_ts;
            sem_post(&mut (*self.shared_memory).output_rx).expect("semaphore failure");
        }
    }

    /// Tries to send a value on this channel. This function doesn't block and
    /// do nothing if the channel is full (in which case it returns `false`).
    #[allow(clippy::missing_panics_doc)]
    pub fn try_send(&mut self, output: &Output<T>) -> bool {
        unsafe {
            if sem_getvalue(&mut (*self.shared_memory).output_tx)
                .expect("semaphore failure")
                > 0
            {
                self.send(output);
                true
            } else {
                false
            }
        }
    }
}

fn serialize_message<T>(
    buf: &mut [u8],
    scratch: &mut Option<FallbackScratch<HeapScratch<SCRATCH_SIZE>, AllocScratch>>,
    value: &T,
) where
    T: Archive + for<'a> Serialize<SharedSerializer<'a>> + Debug,
{
    let mut serializer = CompositeSerializer::new(
        BufferSerializer::new(&mut buf[mem::size_of::<usize>()..]),
        scratch.take().unwrap(),
        SharedSerializeMap::new(), // reuse of this map doesn't work
    );
    serializer
        .serialize_value(value)
        .expect("failed to serialize an IPC message");
    let size = serializer.pos();
    let (_, c, _) = serializer.into_components();
    buf[..mem::size_of::<usize>()].copy_from_slice(&size.to_ne_bytes());
    *scratch = Some(c);
}

unsafe fn deserialize_message<T>(buf: &[u8]) -> &T::Archived
where
    T: Archive + for<'a> Serialize<SharedSerializer<'a>>,
{
    let size = usize::from_ne_bytes(buf[..mem::size_of::<usize>()].try_into().unwrap());
    let bytes = &buf[mem::size_of::<usize>()..mem::size_of::<usize>() + size];
    unsafe { rkyv::archived_root::<T>(bytes) }
}

fn set_init_state<T>(addr: usize, init_state: &T)
where
    T: SharedPort + Debug + Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T as Archive>::Archived: Deserialize<T, Infallible>,
    T::Input: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    T::Output: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T::Output as Archive>::Archived: Deserialize<T::Output, SharedDeserializeMap>,
{
    let mut scratch = Some(FallbackScratch::default());
    unsafe {
        let shared_memory = addr as *mut SharedMemory<T>;
        serialize_message((*shared_memory).init_state(), &mut scratch, init_state);
    }
}

fn spawn_shared_tx_task<T>(
    mut tx: InnerTx<T>,
    addr: usize,
    mut stop_tx_rx: oneshot::Receiver<()>,
) -> task::JoinHandle<InnerTx<T>>
where
    T: SharedPort + Debug + Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T as Archive>::Archived: Deserialize<T, Infallible>,
    T::Input: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    T::Output: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T::Output as Archive>::Archived: Deserialize<T::Output, SharedDeserializeMap>,
{
    task::spawn_local(async move {
        let spawn_sem_wait = || {
            task::spawn_blocking(move || unsafe {
                let shared_memory = addr as *mut SharedMemory<T>;
                sem_wait(&mut (*shared_memory).output_rx).expect("semaphore failure");
            })
        };
        let mut sem_wait = spawn_sem_wait();
        loop {
            if let Either::Left((_, sem_wait)) = select(&mut stop_tx_rx, sem_wait).await
            {
                unsafe {
                    let shared_memory = addr as *mut SharedMemory<T>;
                    sem_post(&mut (*shared_memory).output_rx)
                        .expect("semaphore failure");
                }
                sem_wait.await.unwrap();
                break;
            }
            let (value, source_ts) = unsafe {
                let shared_memory = addr as *mut SharedMemory<T>;
                let archived =
                    deserialize_message::<T::Output>((*shared_memory).output());
                // Reuse of `SharedDeserializeMap` doesn't work
                let value = archived
                    .deserialize(&mut SharedDeserializeMap::new())
                    .unwrap();
                let source_ts = (*shared_memory).output_ts;
                sem_post(&mut (*shared_memory).output_tx).expect("semaphore failure");
                (value, source_ts)
            };
            let mut send = tx.feed(Output { value, source_ts });
            match select(&mut stop_tx_rx, &mut send).await {
                Either::Left((_, _)) | Either::Right((Err(_), _)) => break,
                Either::Right((Ok(result), _)) => result,
            }
            sem_wait = spawn_sem_wait();
        }
        tx
    })
}

fn spawn_shared_rx_task<T>(
    mut rx: InnerRx<T>,
    addr: usize,
    mut stop_rx_rx: oneshot::Receiver<()>,
    mut initial_inputs: InitialInputs,
) -> task::JoinHandle<(InnerRx<T>, InitialInputs)>
where
    T: SharedPort + Debug + Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T as Archive>::Archived: Deserialize<T, Infallible>,
    T::Input: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    T::Output: Archive + for<'a> Serialize<SharedSerializer<'a>>,
    <T::Output as Archive>::Archived: Deserialize<T::Output, SharedDeserializeMap>,
{
    task::spawn_local(async move {
        let spawn_sem_wait = || {
            task::spawn_blocking(move || unsafe {
                let shared_memory = addr as *mut SharedMemory<T>;
                sem_wait(&mut (*shared_memory).input_tx).expect("semaphore failure");
            })
        };
        let mut sem_wait = spawn_sem_wait();
        let mut scratch = Some(FallbackScratch::default());
        loop {
            if let Either::Left((_, sem_wait)) = select(&mut stop_rx_rx, sem_wait).await
            {
                unsafe {
                    let shared_memory = addr as *mut SharedMemory<T>;
                    sem_post(&mut (*shared_memory).input_tx)
                        .expect("semaphore failure");
                }
                sem_wait.await.unwrap();
                break;
            }
            let input = if let Some((input, input_ts)) = initial_inputs.pop() {
                Either::Left((input, input_ts))
            } else {
                match select(&mut stop_rx_rx, rx.next()).await {
                    Either::Left((_, _)) | Either::Right((None, _)) => break,
                    Either::Right((Some(input), _)) => Either::Right(input),
                }
            };
            unsafe {
                let shared_memory = addr as *mut SharedMemory<T>;
                let input_index = (*shared_memory).input_index;
                (*shared_memory).input_count =
                    ((*shared_memory).input_count + 1).min(2);
                (*shared_memory).input_index = ((*shared_memory).input_index + 1) % 2;
                match input {
                    Either::Left((input, input_ts)) => {
                        ptr::copy_nonoverlapping::<u8>(
                            input.as_ptr(),
                            (*shared_memory).input(input_index).as_mut_ptr(),
                            input.len(),
                        );
                        (*shared_memory).input_ts[input_index] = input_ts;
                    }
                    Either::Right(input) => {
                        serialize_message(
                            (*shared_memory).input(input_index),
                            &mut scratch,
                            &input.value,
                        );
                        (*shared_memory).input_ts[input_index] = input.source_ts;
                    }
                }
                sem_post(&mut (*shared_memory).input_rx).expect("semaphore failure");
            }
            sem_wait = spawn_sem_wait();
        }
        (rx, initial_inputs)
    })
}

unsafe fn sem_init(sem: *mut sem_t, pshared: c_int, value: c_uint) -> io::Result<()> {
    let result = unsafe { libc::sem_init(sem, pshared, value) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

unsafe fn sem_destroy(sem: *mut sem_t) -> io::Result<()> {
    let result = unsafe { libc::sem_destroy(sem) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

unsafe fn sem_post(sem: *mut sem_t) -> io::Result<()> {
    let result = unsafe { libc::sem_post(sem) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

unsafe fn sem_getvalue(sem: *mut sem_t) -> io::Result<c_int> {
    let mut value = 0;
    let result = unsafe { libc::sem_getvalue(sem, &mut value) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(value)
    }
}

unsafe fn sem_wait(sem: *mut sem_t) -> io::Result<()> {
    let result = unsafe { libc::sem_wait(sem) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
