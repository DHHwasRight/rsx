//! Reusing render buffers.

use std::sync::{Arc, Mutex};

/// Buffers handed out with their allocation already made.
///
/// # Why a struct rather than a `thread_local!`
///
/// A thread-local pool is faster to reach — no lock — but it composes badly
/// with a work-stealing runtime. A task that takes a buffer on one worker and
/// finishes on another returns it to the wrong pool, so under uneven load the
/// buffers drift onto whichever threads happen to finish requests. An
/// `Arc<BufferPool>` is predictable, can be sized deliberately, and can be
/// swapped for a fake in a test. The lock is held for a `Vec` push or pop and
/// nothing else.
///
/// # What the pool is actually worth
///
/// Only on paths where the rendered bytes are *not* retained. See
/// [`crate::pool::BufferPool::put`] for which paths those are.
pub struct BufferPool {
    idle: Mutex<Vec<String>>,
    /// Capacity a fresh buffer starts with, and the floor a returned one must
    /// still have to be worth keeping.
    buffer_capacity: usize,
    /// How many buffers to hold. Beyond this, a returned buffer is dropped:
    /// a pool that grows without bound is a memory leak with extra steps.
    max_idle: usize,
    /// Buffers larger than this are dropped rather than kept, so one
    /// unusually large page does not pin its allocation forever.
    max_retained_capacity: usize,
}

impl BufferPool {
    /// A pool whose buffers start at `buffer_capacity` bytes.
    ///
    /// Size it at roughly the 90th-percentile page so most renders never
    /// grow the buffer; the cost of being wrong is one realloc.
    pub fn new(buffer_capacity: usize, max_idle: usize) -> Arc<Self> {
        Arc::new(BufferPool {
            idle: Mutex::new(Vec::with_capacity(max_idle)),
            buffer_capacity,
            max_idle,
            // Keeping anything up to eight times the target covers a long
            // tail without letting a single outlier pin megabytes.
            max_retained_capacity: buffer_capacity.saturating_mul(8),
        })
    }

    /// Takes a buffer, empty and with capacity already reserved.
    ///
    /// Never blocks on anything but the pool's own lock, so it is safe to
    /// call from an async context. It does not await and must not be held
    /// across one unless the caller intends to keep the buffer that long.
    pub fn get(&self) -> String {
        match self.lock().pop() {
            // No allocation and no copy: the capacity is already there.
            Some(buffer) => buffer,
            // The pool is empty, which is normal under a burst. Allocating is
            // the correct answer; a pool that blocks waiting for a buffer
            // converts a memory shortage into a latency cliff.
            None => String::with_capacity(self.buffer_capacity),
        }
    }

    /// Returns a buffer for reuse.
    ///
    /// Call this only on paths where the rendered bytes are *not* retained:
    ///
    /// * the response was streamed straight to the socket and nothing kept it;
    /// * the page was rendered but deliberately not cached;
    /// * rendering failed part way and the partial output is discarded.
    ///
    /// A buffer that became an [`Arc<String>`](std::sync::Arc) for the cache
    /// is owned by that `Arc` and must not come back here — there is nothing
    /// to return, since the allocation moved. That is not a leak and not a
    /// flaw in the design: a cached page is rendered once per cache lifetime,
    /// so the pool pays for it once and the hit path never touches a buffer
    /// at all. [`reclaim`](BufferPool::reclaim) covers the case where the
    /// `Arc` later turns out to be the last one.
    pub fn put(&self, mut buffer: String) {
        if buffer.capacity() > self.max_retained_capacity {
            return;
        }
        // Keeps the allocation, drops the bytes.
        buffer.clear();

        let mut idle = self.lock();
        if idle.len() < self.max_idle {
            idle.push(buffer);
        }
    }

    /// Takes the allocation back out of an `Arc` that nobody else holds.
    ///
    /// Returns `true` when the buffer was recovered. Use it where a cache
    /// hands back an evicted entry; with any other reference alive the `Arc`
    /// is left untouched and the answer is `false`.
    pub fn reclaim(&self, rendered: Arc<String>) -> bool {
        match Arc::try_unwrap(rendered) {
            // No copy: the `String` moves out of the `Arc` allocation.
            Ok(buffer) => {
                self.put(buffer);
                true
            }
            Err(_) => false,
        }
    }

    /// How many buffers are idle. For tests and metrics.
    pub fn idle(&self) -> usize {
        self.lock().len()
    }

    /// The capacity a fresh buffer is given.
    pub fn buffer_capacity(&self) -> usize {
        self.buffer_capacity
    }

    /// A poisoned pool is still a usable pool: the data is a list of empty
    /// buffers, and a panic elsewhere says nothing about its contents.
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        self.idle.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl std::fmt::Debug for BufferPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BufferPool")
            .field("idle", &self.idle())
            .field("buffer_capacity", &self.buffer_capacity)
            .field("max_idle", &self.max_idle)
            .finish()
    }
}
