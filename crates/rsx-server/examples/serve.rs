//! The whole path, end to end: a cache miss renders through the pool into an
//! `Arc<String>`; a hit never touches the pool at all.
//!
//! The cache here is a stand-in — about twenty lines of `HashMap` — written
//! only so the signatures it calls are exercised against a real caller. The
//! real one goes in its place without either side changing.
//!
//! Run with `cargo run -p rsx-server --example serve`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rsx_server::{render_owned, render_streaming, BufferPool, Rendered};

rsx::load!("examples/page.rsx");

// ----------------------------------------------------------- the stand-in

/// Stands in for the real crate: L1 only, no TTL, no Redis.
///
/// What matters is the shape of `get_or_insert`, which is what the real one
/// is expected to offer.
#[derive(Default)]
struct Cache {
    entries: Mutex<HashMap<String, Rendered>>,
}

impl Cache {
    /// Generic over the miss producer, so nothing is boxed and the future is
    /// inlined. This is the signature `rsx_server::MissProducer` is written
    /// against.
    async fn get_or_insert<F, Fut>(&self, key: &str, miss: F) -> rsx::Result<Rendered>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = rsx::Result<Rendered>> + Send,
    {
        // Hit: a refcount bump. No render, no buffer, no copy.
        if let Some(hit) = self.entries.lock().unwrap().get(key) {
            return Ok(Arc::clone(hit));
        }

        let rendered = miss().await?;

        // Insert: another refcount bump. The same `Arc` goes to the caller,
        // so the bytes exist once however many times the page is served.
        self.entries
            .lock()
            .unwrap()
            .insert(key.to_string(), Arc::clone(&rendered));

        Ok(rendered)
    }
}

// ------------------------------------------------------------- a database

struct Db;

impl Db {
    async fn user(&self, name: &str) -> User {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        User {
            name: name.to_string(),
            verified: true,
        }
    }

    async fn posts(&self, count: u64) -> Vec<Post> {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        (0..count)
            .map(|i| Post {
                title: format!("Post number {i}"),
                views: i * 37,
            })
            .collect()
    }
}

// ------------------------------------------------------------ the handler

/// A cacheable page.
///
/// Everything is awaited inside the miss closure, and the render below it is
/// synchronous — so no `Writer` is alive at any `.await`.
async fn cached_page(
    cache: &Cache,
    db: &Db,
    pool: &Arc<BufferPool>,
    name: &str,
) -> rsx::Result<Rendered> {
    cache
        .get_or_insert(name, || async {
            let user = db.user(name).await;
            let posts = db.posts(8).await;

            // Sync from here down. The buffer becomes the `Arc` and does not
            // come back to the pool.
            render_owned(
                pool,
                Page {
                    user: &user,
                    posts: &posts,
                },
            )
        })
        .await
}

/// An uncacheable page: straight to the socket, nothing retained.
fn streamed_page(
    pool: &BufferPool,
    user: &User,
    posts: &[Post],
    out: &mut Vec<u8>,
) -> rsx::Result<()> {
    render_streaming(pool, Page { user, posts }, out)
}

#[tokio::main]
async fn main() -> rsx::Result<()> {
    // Sized at roughly a typical page, so most renders never grow it.
    let pool = BufferPool::new(16 * 1024, 64);
    let cache = Cache::default();
    let db = Db;

    let first = cached_page(&cache, &db, &pool, "Max").await?;
    println!(
        "miss  : {} bytes, pool now holds {}",
        first.len(),
        pool.idle()
    );

    let second = cached_page(&cache, &db, &pool, "Max").await?;
    println!(
        "hit   : {} bytes, pool now holds {}",
        second.len(),
        pool.idle()
    );

    // The same allocation, not an equal one: a hit copies nothing.
    println!(
        "same  : {} (pointer equality, so the hit copied nothing)",
        Arc::ptr_eq(&first, &second)
    );
    println!("refs  : {}", Arc::strong_count(&first));

    // A page nobody keeps: the buffer goes back.
    let user = db.user("Max").await;
    let posts = db.posts(3).await;
    let mut socket = Vec::new();
    streamed_page(&pool, &user, &posts, &mut socket)?;
    println!(
        "stream: {} bytes, pool now holds {}",
        socket.len(),
        pool.idle()
    );

    // An evicted entry nobody else holds gives its allocation back.
    drop(second);
    cache.entries.lock().unwrap().clear();
    let recovered = pool.reclaim(first);
    println!(
        "evict : reclaimed = {recovered}, pool now holds {}",
        pool.idle()
    );

    Ok(())
}
