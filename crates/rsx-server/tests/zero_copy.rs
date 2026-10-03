//! The claims the design rests on, checked rather than asserted in prose.
//!
//! Most of these compare *allocation addresses*: if the pointer a pooled
//! buffer started with is the pointer the `Arc` ends up with, the page bytes
//! were never copied. A comment saying so would rot; this does not.

use std::sync::Arc;

use rsx::Template;
use rsx_server::{render_borrowed, render_owned, render_streaming, BufferPool};

rsx::load!("examples/page.rsx");

fn fixture() -> (User, Vec<Post>) {
    (
        User {
            name: "Max".to_string(),
            verified: true,
        },
        (0..4)
            .map(|i| Post {
                title: format!("Post {i}"),
                views: i,
            })
            .collect(),
    )
}

#[test]
fn the_pooled_allocation_becomes_the_arc_without_a_copy() {
    let pool = BufferPool::new(16 * 1024, 8);
    let (user, posts) = fixture();

    // The address of the buffer the pool will hand out.
    let buffer = pool.get();
    let before = buffer.as_ptr();
    pool.put(buffer);

    let rendered = render_owned(
        &pool,
        Page {
            user: &user,
            posts: &posts,
        },
    )
    .unwrap();

    assert_eq!(
        before,
        rendered.as_ptr(),
        "the page bytes were copied somewhere between the pool and the Arc"
    );
}

#[test]
fn a_render_that_is_not_retained_returns_its_buffer() {
    let pool = BufferPool::new(8 * 1024, 8);
    let (user, posts) = fixture();
    assert_eq!(pool.idle(), 0);

    let length = render_borrowed(
        &pool,
        Page {
            user: &user,
            posts: &posts,
        },
        |html| {
            assert!(html.contains("Max"));
            html.len()
        },
    )
    .unwrap();

    assert!(length > 0);
    assert_eq!(pool.idle(), 1, "the buffer should have come back");
}

#[test]
fn the_buffer_survives_a_failed_render() {
    // An error is not a reason to throw an allocation away.
    struct Fails;
    impl Template for Fails {
        fn render(self, w: &mut rsx::Writer<'_>) -> rsx::Result {
            w.raw("<p>partial")?;
            Err(rsx::Error::custom("the database went away"))
        }
    }

    let pool = BufferPool::new(1024, 4);
    assert!(render_borrowed(&pool, Fails, |_| ()).is_err());
    assert_eq!(pool.idle(), 1, "a failed render still owes its buffer back");
}

#[test]
fn streaming_borrows_a_buffer_and_gives_it_back() {
    let pool = BufferPool::new(4 * 1024, 4);
    let (user, posts) = fixture();
    let mut sink = Vec::new();

    render_streaming(
        &pool,
        Page {
            user: &user,
            posts: &posts,
        },
        &mut sink,
    )
    .unwrap();

    assert!(sink.starts_with(b"<!DOCTYPE html>"));
    assert_eq!(pool.idle(), 1);
}

#[test]
fn reclaim_recovers_an_allocation_nobody_else_holds() {
    let pool = BufferPool::new(1024, 4);
    let (user, posts) = fixture();
    let rendered = render_owned(
        &pool,
        Page {
            user: &user,
            posts: &posts,
        },
    )
    .unwrap();
    let address = rendered.as_ptr();

    // A second holder — the cache — keeps it alive.
    let in_cache = Arc::clone(&rendered);
    assert!(
        !pool.reclaim(rendered),
        "must not take a buffer still in use"
    );
    assert_eq!(pool.idle(), 0);

    // Once it is the last one, the allocation comes home.
    assert!(pool.reclaim(in_cache));
    assert_eq!(pool.idle(), 1);
    // Bound, not a temporary: `pool.get().as_ptr()` would compare a pointer
    // into a `String` already dropped at the end of the statement.
    let recovered = pool.get();
    assert_eq!(
        recovered.as_ptr(),
        address,
        "a different allocation came back"
    );
}

#[test]
fn the_pool_refuses_to_grow_without_bound() {
    let pool = BufferPool::new(1024, 2);
    for _ in 0..10 {
        pool.put(String::with_capacity(1024));
    }
    assert_eq!(pool.idle(), 2, "the pool kept more than it was told to");
}

#[test]
fn an_outsized_buffer_is_dropped_rather_than_pinned() {
    // One unusual page should not cost the process its memory for good.
    let pool = BufferPool::new(1024, 4);
    pool.put(String::with_capacity(1024 * 64));
    assert_eq!(pool.idle(), 0);
}

#[test]
fn an_empty_pool_allocates_rather_than_blocking() {
    let pool = BufferPool::new(2048, 4);
    let buffer = pool.get();
    assert!(buffer.is_empty());
    assert!(buffer.capacity() >= 2048);
}

#[test]
fn nested_components_share_one_buffer() {
    // `Page` contains four `Card`s. If each allocated, the pool would see
    // more than one buffer leave.
    let pool = BufferPool::new(16 * 1024, 8);
    let (user, posts) = fixture();
    let buffer = pool.get();
    let address = buffer.as_ptr();
    pool.put(buffer);

    let html = render_owned(
        &pool,
        Page {
            user: &user,
            posts: &posts,
        },
    )
    .unwrap();

    assert_eq!(html.matches("class=\"card\"").count(), 4);
    assert_eq!(
        address,
        html.as_ptr(),
        "a nested component took its own buffer"
    );
}

#[test]
fn a_writer_can_be_held_across_an_await() {
    // Not how the design works — data is fetched first — but the type has to
    // allow it, or adding an async seam later means changing every
    // signature. `Send` is what `tokio::spawn` requires.
    fn assert_send<T: Send>() {}
    assert_send::<rsx::Writer<'static>>();
    assert_send::<BufferPool>();
    assert_send::<rsx_server::Rendered>();
}
