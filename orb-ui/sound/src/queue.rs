use super::{alsa_to_io_error, eventfd, read, select, write, Device, HwParams};
use futures::prelude::*;
use libc::{
    c_int, c_void, fd_set, suseconds_t, time_t, timeval, EFD_CLOEXEC, FD_ISSET, FD_SET,
    FD_ZERO,
};
use std::{
    ffi::CString,
    io,
    io::prelude::*,
    mem::{self, MaybeUninit},
    pin::Pin,
    ptr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Waker},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Sound queue.
pub struct Queue {
    buffer: SharedBuffer,
    queue_event: c_int,
    cancel_event: c_int,
    counter: AtomicU64,
    thread: Option<thread::JoinHandle<()>>,
}

/// Builder-pattern for [`Queue`] elements.
#[derive(Default)]
pub struct SoundBuilder<'a> {
    queue: Option<&'a Queue>,
    name: String,
    reader: Option<Box<dyn Reader>>,
    cancel_all: bool,
    priority: u8,
    max_delay: Duration,
    volume: f64,
}

/// Future returned by [`SoundBuilder::push`] method.
pub struct SoundFuture {
    state: Option<SharedState>,
}

struct Sound {
    id: u64,
    reader: Box<dyn Reader>,
    name: String,
    deadline: Duration,
    volume: f64,
    state: SharedState,
}

enum State {
    Pending,
    Waiting(Waker),
    Done(bool),
}

type SharedBuffer = Arc<Mutex<Option<Vec<Sound>>>>;
type SharedState = Arc<Mutex<State>>;

trait Reader: Read + Seek + Send + 'static {}

impl<T: Read + Seek + Send + 'static> Reader for T {}

impl Queue {
    /// Spawns a new thread for the sound queue and returns `Queue` handle.
    pub fn spawn(card_name: &str) -> io::Result<Self> {
        let queue_event = unsafe { eventfd(1, EFD_CLOEXEC)? };
        let cancel_event = unsafe { eventfd(0, EFD_CLOEXEC)? };
        let buffer = Arc::new(Mutex::new(Some(Vec::new())));
        let buffer2 = Arc::clone(&buffer);
        let card_name = card_name.to_string();
        let counter = AtomicU64::new(u64::from(u8::MAX) << 56 ^ u64::MAX);
        let thread = thread::Builder::new()
            .name("sound-queue".into())
            .spawn(move || {
                if let Ok(title) = CString::new("sound-queue") {
                    unsafe { libc::prctl(libc::PR_SET_NAME, title.as_ptr(), 0, 0, 0) };
                }
                loop {
                    match queue_loop(&card_name, &buffer2, queue_event, cancel_event) {
                        Ok(()) => break,
                        Err(err) => {
                            tracing::error!("sound thread exited with error: {}", err);
                        }
                    }
                }
            })
            .expect("failed to spawn thread");
        Ok(Self {
            buffer,
            queue_event,
            cancel_event,
            counter,
            thread: Some(thread),
        })
    }

    /// Returns a builder object for inserting a new queue element.
    #[must_use]
    pub fn sound<T: Read + Seek + Send + 'static>(
        &self,
        reader: Option<T>,
        name: String,
    ) -> SoundBuilder {
        SoundBuilder {
            queue: Some(self),
            name,
            reader: reader.map(|reader| Box::new(reader) as _),
            cancel_all: false,
            priority: 0,
            max_delay: Duration::MAX,
            volume: 1.0,
        }
    }

    fn push(&self, sound: Sound, cancel_all: bool) -> io::Result<()> {
        {
            let mut guard = self.buffer.lock().unwrap();
            let buffer = guard.as_mut().unwrap();
            if cancel_all {
                for sound in mem::take(buffer) {
                    let state = &mut *sound.state.lock().unwrap();
                    if let State::Waiting(waker) = mem::replace(state, State::Pending) {
                        waker.wake();
                    }
                    *state = State::Done(false);
                }
                unsafe {
                    let arg: u64 = 1;
                    write(
                        self.cancel_event,
                        ptr::addr_of!(arg).cast::<c_void>(),
                        mem::size_of_val(&arg),
                    )?;
                }
            }
            let i = buffer
                .binary_search_by_key(&sound.id, |sound| sound.id)
                .unwrap_err();
            buffer.insert(i, sound);
        }
        unsafe {
            let arg: u64 = 1;
            write(
                self.queue_event,
                ptr::addr_of!(arg).cast::<c_void>(),
                mem::size_of_val(&arg),
            )?;
        }
        Ok(())
    }

    /// Returns whether the queue is empty.
    pub fn empty(&self) -> bool {
        self.buffer.lock().unwrap().as_ref().unwrap().is_empty()
    }
}

impl SoundBuilder<'_> {
    /// Sets the `cancel_all` flag. Immediately stops any currently playing sounds.
    #[must_use]
    pub fn cancel_all(mut self) -> Self {
        self.cancel_all = true;
        self
    }

    /// Sets the sound priority. Sounds with higher priorities take precedence
    /// over sounds with lower priorities.
    #[must_use]
    pub fn priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Sets the maximum delay before playing the sound. If the delay is
    /// greater, the sound will not be played.
    #[must_use]
    pub fn max_delay(mut self, max_delay: Duration) -> Self {
        self.max_delay = max_delay;
        self
    }

    /// Sets the volume multiplier for this sound.
    #[must_use]
    pub fn volume(mut self, volume: f64) -> Self {
        self.volume = volume;
        self
    }

    /// Inserts the sound to the queue returning a future that resolves when the
    /// sound has left the queue. The boolean output represents whether the
    /// sound has been played.
    ///
    /// The returned future can be safely dropped if there is no need to wait
    /// the end of the sound.
    pub fn push(self) -> io::Result<SoundFuture> {
        let Self {
            queue,
            name,
            reader,
            cancel_all,
            priority,
            max_delay,
            volume,
        } = self;
        let Some(queue) = queue else {
            return Ok(SoundFuture { state: None });
        };
        let Some(reader) = reader else {
            return Ok(SoundFuture { state: None });
        };
        let counter = queue.counter.fetch_sub(1, Ordering::Relaxed);
        assert!(
            counter != 0,
            "the queue has played more than 10^16 sounds, how is this possible?"
        );
        let id = u64::from(priority) << 56 | counter;
        let deadline = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?
            .saturating_add(max_delay);
        let state = Arc::new(Mutex::new(State::Pending));
        let sound = Sound {
            id,
            name,
            reader,
            deadline,
            state: Arc::clone(&state),
            volume,
        };
        queue.push(sound, cancel_all)?;
        Ok(SoundFuture { state: Some(state) })
    }
}

impl Future for SoundFuture {
    type Output = bool;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(state) = &self.state else {
            return Poll::Ready(false);
        };
        let state = &mut *state.lock().unwrap();
        match state {
            State::Pending => {
                *state = State::Waiting(cx.waker().clone());
                Poll::Pending
            }
            State::Waiting(_) => Poll::Pending,
            State::Done(played) => Poll::Ready(*played),
        }
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        {
            let mut buffer = self.buffer.lock().unwrap();
            *buffer = None;
        }
        unsafe {
            let arg: u64 = 1;
            write(
                self.queue_event,
                ptr::addr_of!(arg).cast::<c_void>(),
                mem::size_of_val(&arg),
            )
            .unwrap();
        }
        self.thread.take().unwrap().join().unwrap();
    }
}

impl Read for Sound {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

impl Seek for Sound {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.reader.seek(pos)
    }
}

fn queue_loop(
    card_name: &str,
    queue: &SharedBuffer,
    queue_event: c_int,
    cancel_event: c_int,
) -> io::Result<()> {
    let mut device = Device::open(card_name).map_err(alsa_to_io_error)?;
    let mut hw_params = HwParams::new().map_err(alsa_to_io_error)?;
    loop {
        let sound = {
            if let Some(queue) = queue.lock().unwrap().as_mut() {
                queue.pop()
            } else {
                break Ok(());
            }
        };
        if let Some(mut sound) = sound {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            let play = now < sound.deadline;
            if play {
                let volume = sound.volume;
                // Reset any previously set cancel event.
                cancellable_sleep(Duration::ZERO, cancel_event)?;
                let start = Instant::now();
                let mut duration =
                    device.play_wav(&mut sound, &mut hw_params, volume)?;
                // In case the sound is longer than the buffer.
                duration = duration.saturating_sub(start.elapsed());
                let cancelled = cancellable_sleep(duration, cancel_event)?;
                if cancelled {
                    tracing::info!("Sound {} cancelled", sound.name);
                    device.drop().map_err(alsa_to_io_error)?;
                } else {
                    device.drain().map_err(alsa_to_io_error)?;
                }
            } else {
                tracing::info!(
                    "Skipping sound {} because it's late for {:.1} seconds",
                    sound.name,
                    now.saturating_sub(sound.deadline).as_secs_f32()
                );
            }
            let state = &mut *sound.state.lock().unwrap();
            if let State::Waiting(waker) = mem::replace(state, State::Pending) {
                waker.wake();
            }
            *state = State::Done(play);
        } else {
            unsafe {
                let mut arg: u64 = 0;
                read(
                    queue_event,
                    ptr::addr_of_mut!(arg).cast::<c_void>(),
                    mem::size_of_val(&arg),
                )?;
            }
        }
    }
}

fn cancellable_sleep(dur: Duration, event: c_int) -> io::Result<bool> {
    let mut tv = timeval {
        tv_sec: dur.as_secs() as time_t,
        tv_usec: suseconds_t::from(dur.subsec_micros()),
    };
    unsafe {
        #[allow(invalid_value, clippy::uninit_assumed_init)]
        let mut fd_set: fd_set = MaybeUninit::uninit().assume_init();
        FD_ZERO(&mut fd_set);
        FD_SET(event, &mut fd_set);
        let n = select(
            event + 1,
            &mut fd_set,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut tv,
        )?;
        let cancelled = n > 0 && FD_ISSET(event, &fd_set);
        if cancelled {
            let mut arg: u64 = 0;
            read(
                event,
                ptr::addr_of_mut!(arg).cast::<c_void>(),
                mem::size_of_val(&arg),
            )?;
        }
        Ok(cancelled)
    }
}
