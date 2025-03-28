use std::{
    collections::{HashSet, VecDeque},
    fmt::Debug,
    hash::Hash,
    ops::Deref,
    sync::Arc,
};

/// Anything that acts like an `Arc<T>`.
pub trait ArcIsh:
    Clone
    + AsRef<<Self as ArcIsh>::Target>
    + Deref<Target = <Self as ArcIsh>::Target>
    + From<<Self as ArcIsh>::Target>
{
    type Target;

    fn into_inner(self) -> Option<<Self as ArcIsh>::Target>;
    fn strong_count(this: &Self) -> usize;
}

impl<T> ArcIsh for Arc<T> {
    type Target = T;

    fn into_inner(self) -> Option<<Self as ArcIsh>::Target> {
        Arc::into_inner(self)
    }

    fn strong_count(this: &Self) -> usize {
        Arc::strong_count(this)
    }
}

#[repr(transparent)]
struct Ptr<T>(*const T);

impl<T> Ptr<T> {
    const fn new(pointee: &T) -> Self {
        Self(pointee as *const T)
    }
}

impl<T> Debug for Ptr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T> PartialEq for Ptr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Eq for Ptr<T> {}

impl<T> Hash for Ptr<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}

/// Safety: We only compare the values of the pointers not their contents.
unsafe impl<T> Send for Ptr<T> {}

/// Safety: We only use the values of the pointers not their contents.
unsafe impl<T> Sync for Ptr<T> {}

/// Intelligently pools `Arc<T>`s, allowing their contents to be reused and written
/// to once all outstanding references have been dropped.
///
/// This is potentially more efficient than repeatedly allocating large buffers
/// and then dropping those buffers when the arc is dropped.
pub struct ArcPool<A: ArcIsh> {
    /// Holds arcs swapped out of the tokio channel. Popped only when strong count
    /// indicates no further references.
    arc_pool: VecDeque<A>,
    tracked: HashSet<Ptr<<A as ArcIsh>::Target>>,
    /// Holds arcs that were promoted to `T` after checking their count.
    raw_pool: Vec<<A as ArcIsh>::Target>,
}

impl<A: ArcIsh> ArcPool<A> {
    pub fn new() -> Self {
        let arc_pool = VecDeque::new();
        let raw_pool = Vec::new();
        let tracked = HashSet::new();

        Self {
            arc_pool,
            raw_pool,
            tracked,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let arc_pool = VecDeque::with_capacity(capacity);
        let raw_pool = Vec::with_capacity(capacity);
        let tracked = HashSet::with_capacity(capacity);

        Self {
            arc_pool,
            raw_pool,
            tracked,
        }
    }

    /// Reaps up to `max_arcs` number of Arcs from the Arc pool.
    pub fn garbage_collect(&mut self, max_arcs: usize) -> usize {
        let mut num_popped = 0;
        for _ in 0..max_arcs {
            let Some(candidate) = self.arc_pool.pop_front() else {
                return num_popped;
            };
            let ptr = Ptr::new(candidate.as_ref());
            // TODO: demote to debug_assert once we are confident in its correctness
            assert!(
                { self.tracked.contains(&ptr) },
                "sanity: all arcs should also exist in `self.tracked`"
            );
            if A::strong_count(&candidate) != 1 {
                // there are other references, return to queue.
                self.arc_pool.push_back(candidate);
                continue;
            }
            // No other references, move to `out`.
            let inner = A::into_inner(candidate)
                .expect("we just checked the reference count, should be infallible");
            self.raw_pool.push(inner);
            self.tracked.remove(&ptr);
            num_popped += 1;
        }

        num_popped
    }

    /// Returns `None` if there are no unused arcs. Try running
    /// [`Self::garbage_collect`] to attempt to find unused arcs.
    pub fn pop(&mut self) -> Option<<A as ArcIsh>::Target> {
        self.raw_pool.pop()
    }

    /// Starts tracking the `arc` in the pool.
    pub fn track(&mut self, arc: A) {
        let ptr = Ptr::new(arc.as_ref());
        if self.tracked.contains(&ptr) {
            // already tracking the arc
            return;
        }
        self.tracked.insert(ptr);
        self.arc_pool.push_back(arc);
    }
}

impl<A: ArcIsh> Debug for ArcPool<A>
where
    A: Debug,
    <A as ArcIsh>::Target: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(std::any::type_name::<Self>())
            .field("arc_pool", &self.arc_pool)
            .field("tracked", &self.tracked)
            .field("raw_pool", &self.raw_pool)
            .finish()
    }
}

impl<A: ArcIsh> Default for ArcPool<A> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_tracked_prevents_memory_leak() {
        let mut pool = ArcPool::new();
        assert_eq!(pool.tracked.len(), 0, "sanity");
        assert_eq!(pool.arc_pool.len(), 0, "sanity");
        assert_eq!(pool.raw_pool.len(), 0, "sanity");
        let a = Arc::new(vec![0; 16]);
        let a_clone = a.clone();

        pool.track(a);
        assert_eq!(pool.tracked.len(), 1, "sanity");
        assert_eq!(pool.arc_pool.len(), 1, "sanity");
        assert_eq!(pool.raw_pool.len(), 0);

        pool.track(a_clone); // should detect that it is already tracked.
        assert_eq!(pool.tracked.len(), 1);
        assert_eq!(pool.arc_pool.len(), 1);
        assert_eq!(pool.raw_pool.len(), 0);

        // Double checks that memory leaks aren't happening
        assert_eq!(pool.garbage_collect(usize::MAX), 1);
        assert_eq!(pool.tracked.len(), 0);
        assert_eq!(pool.arc_pool.len(), 0, "sanity");
        assert_eq!(pool.raw_pool.len(), 1);
    }
}
