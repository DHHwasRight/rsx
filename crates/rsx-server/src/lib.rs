//! Serving rsx templates: pooled buffers, and the boundary with a cache.
//!
//! The template compiler and runtime live in [`rsx`] and know nothing about
//! servers. This crate adds the two things a server needs — somewhere for the
//! bytes to go, and a shape the cache can call — without putting either in
//! the runtime, so a program that renders one page at build time does not
//! acquire a buffer pool.
//!
//! # The zero-copy contract
//!
//! Rendering never builds a string per node. A template writes into a
//! caller-provided [`rsx::Writer`], and a template that contains other
//! templates passes that same `Writer` down, so a page of fifty components
//! fills one buffer. The compiler enforces this: there is no way to write a
//! component that returns a `String`, because [`rsx::Template::render`] takes
//! the writer and returns `()`.
//!
//! What an implementor must not do, if hand-writing one:
//!
//! * build an intermediate `String` and write it in one go — write the parts;
//! * call `render_to_string` on a child — pass the `Writer` through;
//! * hold the `Writer` past the call — it borrows the caller's buffer.
//!
//! # Where the bytes are copied, and where they are not
//!
//! | Step | Cost |
//! | --- | --- |
//! | pool → `Writer` | move of (ptr, len, cap) |
//! | component → component | none; one buffer throughout |
//! | static markup → buffer | `memcpy` of the literal, unavoidable |
//! | `{ expr }` → buffer | formatted in place through an escaping adapter |
//! | `Writer` → `String` | move |
//! | `String` → `Arc<String>` | 16-byte header allocated; **content not copied** |
//! | `Arc<String>` → cache L1 | refcount bump |
//! | `Arc<String>` → cache L2 (Redis) | **copy**, and a serialisation, in the cache crate |
//! | `Arc<String>` → socket | **copy**, into the kernel |
//!
//! Two copies are unavoidable and both are at an edge of the process. Nothing
//! between a template and the response copies the page.
//!
//! # Async
//!
//! Rendering is synchronous, and that is a deliberate choice rather than a
//! limitation: the data is awaited first, then written. The miss boundary is
//! still `async`, so the awaiting happens inside it:
//!
//! ```ignore
//! let page = cache
//!     .get_or_insert(key, || async {
//!         let user = db.user(id).await?;          // the await lives here
//!         render_owned(&pool, Page { user: &user }) // sync from here down
//!     })
//!     .await?;
//! ```
//!
//! So a `Writer` is never held across an `.await`. It would be sound if it
//! were — it is `Send`, and the test suite asserts that so it stays that way
//! — but not holding it means the render is a tight loop with no state
//! machine around it, and no component needs to be rewritten as a future.
//!
//! If a page genuinely must interleave I/O with markup, split it: render the
//! parts either side of the await and pass the finished pieces in.
//!
//! # What this crate deliberately does not do
//!
//! **Coalescing concurrent misses.** Eight requests for a cold key can all
//! miss before any of them inserts, and then all eight render. Whether that
//! is prevented — single-flight, request coalescing, whatever the cache
//! calls it — is the cache's decision, because only it knows when an entry
//! becomes visible. Worth confirming before relying on it: a page that takes
//! 20 ms to render and gets 500 requests a second will be rendered about ten
//! times per cache fill without it.
//!
//! **Deciding what is cacheable.** Nothing here inspects a page. Use
//! [`render_owned`] for what should be kept and [`render_streaming`] for
//! what should not; the difference is which one returns the buffer.

pub mod pool;
pub mod render;

pub use pool::BufferPool;
pub use render::{
    boxed_miss, render_borrowed, render_owned, render_streaming, sync_miss, BoxedMiss,
    MissProducer, Rendered, RenderedFuture,
};
