//! Rendering into a pooled buffer, and the boundary with a cache.

use std::future::Future;
use std::sync::Arc;

use rsx::{Result, Template, Writer};

use crate::pool::BufferPool;

/// A rendered page, ready to be cached and written to a socket.
///
/// `Arc<String>` rather than `Arc<str>` deliberately. Converting a `String`
/// to an `Arc<str>` copies every byte, because the length has to move into
/// the allocation header. `Arc<String>` costs one extra pointer hop when
/// reading and copies nothing.
pub type Rendered = Arc<String>;

/// Renders into a pooled buffer and gives up the buffer to the result.
///
/// The allocation travels: pool → `Writer` → `String` → `Arc`, and is never
/// copied on the way. The buffer does not return to the pool, because it no
/// longer exists separately from the `Arc` — see [`BufferPool::put`].
///
/// Use this on a cache miss, where the bytes outlive the request.
pub fn render_owned(pool: &BufferPool, template: impl Template) -> Result<Rendered> {
    // Zero-copy: an existing allocation, emptied.
    let buffer = pool.get();

    // Zero-copy: `String` → `Vec<u8>` is a move of (ptr, len, cap).
    let mut writer = Writer::from_buffer(buffer.into_bytes());

    // The render itself. Nested templates write into this same `Writer`, so
    // a page composed of twenty components still fills one buffer.
    template.render(&mut writer)?;

    // Zero-copy: `Vec<u8>` → `String` is the same move back.
    let rendered = writer.into_string()?;

    // Zero-copy *of the content*: `Arc::new` allocates a 16-byte header and
    // moves the `String` into it. The page bytes are not touched.
    Ok(Arc::new(rendered))
}

/// Renders into a pooled buffer and hands the buffer back.
///
/// For responses nothing retains: the body is handed to `consume` and the
/// allocation returns to the pool whatever `consume` does, including panic.
///
/// Use this on paths that do not cache.
pub fn render_borrowed<T>(
    pool: &BufferPool,
    template: impl Template,
    consume: impl FnOnce(&str) -> T,
) -> Result<T> {
    let buffer = pool.get();
    let mut writer = Writer::from_buffer(buffer.into_bytes());

    // A failed render still has an allocation worth keeping, so the buffer is
    // recovered before the error leaves.
    let rendered = match template.render(&mut writer) {
        Ok(()) => writer.into_string(),
        Err(error) => {
            if let Ok(bytes) = writer.into_bytes() {
                pool.put(unsafe_utf8(bytes));
            }
            return Err(error);
        }
    }?;

    // Borrowed, not copied: `consume` sees the bytes in place.
    let outcome = consume(rendered.as_str());
    pool.put(rendered);
    Ok(outcome)
}

/// Renders straight into a sink, in chunks, with a pooled buffer behind it.
///
/// Nothing is retained and the whole page is never held in memory at once.
/// This is the cheapest path and the right one for anything uncacheable.
pub fn render_streaming(
    pool: &BufferPool,
    template: impl Template,
    sink: &mut (dyn std::io::Write + Send),
) -> Result<()> {
    // The chunk buffer comes from the pool like any other, and goes back:
    // nothing on this path is retained, so this is where pooling pays.
    let buffer = pool.get();
    let mut writer = Writer::streaming_with(sink, buffer.into_bytes());

    let rendered = template.render(&mut writer);

    // Unavoidable copy — into the kernel, as each chunk is written. Every
    // byte above this line stayed where it was first put.
    let (buffer, flushed) = writer.finish_buffer();
    pool.put(unsafe_utf8(buffer));

    rendered.and(flushed)
}

/// Reinterprets bytes this crate itself produced.
///
/// Only reached on the error path of [`render_borrowed`], where the bytes
/// came from a `Writer` that was never used as an `io::Write`.
fn unsafe_utf8(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_default()
}

// ---------------------------------------------------------------- the cache

/// What a cache calls when it does not have the page.
///
/// The cache crate owns the lookup, the L1/L2 tiering and the eviction; this
/// is the whole of the surface it needs from here. Shaped as a bound rather
/// than a trait object so the future can be inlined and nothing is boxed:
///
/// ```ignore
/// pub async fn get_or_insert<F, Fut>(&self, key: &str, miss: F) -> Result<Arc<String>>
/// where
///     F: FnOnce() -> Fut + Send,
///     Fut: Future<Output = rsx::Result<Arc<String>>> + Send;
/// ```
///
/// A cache that instead wants a trait object can take `Box<dyn MissProducer>`
/// built from [`boxed_miss`]; a cache that wants a plain `dyn Fn` can call
/// [`sync_miss`].
pub trait MissProducer: Send {
    /// The future this produces. Named so a caller can bound it.
    type Future: Future<Output = Result<Rendered>> + Send;

    /// Produces the page. Called at most once per miss.
    fn produce(self) -> Self::Future;
}

impl<F, Fut> MissProducer for F
where
    F: FnOnce() -> Fut + Send,
    Fut: Future<Output = Result<Rendered>> + Send,
{
    type Future = Fut;

    fn produce(self) -> Fut {
        self()
    }
}

/// Adapts a synchronous render into the async miss shape.
///
/// The closure runs on the calling task. Keep it to rendering: it must not
/// block, because there is no await point for the runtime to work with.
pub fn sync_miss<F>(produce: F) -> impl FnOnce() -> std::future::Ready<Result<Rendered>> + Send
where
    F: FnOnce() -> Result<Rendered> + Send,
{
    move || std::future::ready(produce())
}

/// A rendered page arriving later. The erased form of a miss's future.
pub type RenderedFuture = std::pin::Pin<Box<dyn Future<Output = Result<Rendered>> + Send>>;

/// A miss producer with its type erased, for a cache that takes one.
pub type BoxedMiss = Box<dyn FnOnce() -> RenderedFuture + Send>;

/// Boxes a miss producer for a cache that is not generic over it.
///
/// Costs one allocation per miss, which is noise next to the render it wraps.
pub fn boxed_miss<F, Fut>(produce: F) -> BoxedMiss
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<Rendered>> + Send + 'static,
{
    Box::new(move || Box::pin(produce()))
}
