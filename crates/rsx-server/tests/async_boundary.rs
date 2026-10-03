//! The miss boundary, against a real runtime.
//!
//! The thing worth proving is that a handler built this way is `Send`, so
//! `tokio::spawn` accepts it. That is the constraint that makes a non-`Send`
//! `Writer` fatal rather than inconvenient, and it is invisible until
//! something tries to spawn.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rsx_server::{render_owned, sync_miss, BufferPool, Rendered};

rsx::load!("examples/page.rsx");

/// The shape the real cache crate is expected to offer.
#[derive(Default)]
struct Cache {
    entries: Mutex<HashMap<String, Rendered>>,
    misses: AtomicUsize,
}

impl Cache {
    async fn get_or_insert<F, Fut>(&self, key: &str, miss: F) -> rsx::Result<Rendered>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = rsx::Result<Rendered>> + Send,
    {
        if let Some(hit) = self.entries.lock().unwrap().get(key) {
            return Ok(Arc::clone(hit));
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        let rendered = miss().await?;
        self.entries
            .lock()
            .unwrap()
            .insert(key.to_string(), Arc::clone(&rendered));
        Ok(rendered)
    }
}

fn fixture() -> (User, Vec<Post>) {
    (
        User {
            name: "Max".to_string(),
            verified: true,
        },
        vec![Post {
            title: "One".to_string(),
            views: 1,
        }],
    )
}

#[tokio::test]
async fn a_miss_renders_and_a_hit_does_not() {
    let pool = BufferPool::new(8 * 1024, 8);
    let cache = Cache::default();

    for _ in 0..3 {
        let page = cache
            .get_or_insert("home", || async {
                // Awaiting here is fine: no `Writer` exists yet.
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                let (user, posts) = fixture();
                // Synchronous from here, so nothing borrows across an await.
                render_owned(
                    &pool,
                    Page {
                        user: &user,
                        posts: &posts,
                    },
                )
            })
            .await
            .unwrap();
        assert!(page.contains("Max"));
    }

    assert_eq!(
        cache.misses.load(Ordering::Relaxed),
        1,
        "rendered more than once"
    );
}

#[tokio::test]
async fn the_handler_future_is_spawnable() {
    // The whole reason `Writer` had to become `Send`.
    let pool = BufferPool::new(8 * 1024, 8);
    let cache = Arc::new(Cache::default());

    let mut tasks = Vec::new();
    for i in 0..8 {
        let cache = Arc::clone(&cache);
        let pool = Arc::clone(&pool);
        tasks.push(tokio::spawn(async move {
            let key = format!("page-{}", i % 2);
            cache
                .get_or_insert(&key, || async {
                    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                    let (user, posts) = fixture();
                    render_owned(
                        &pool,
                        Page {
                            user: &user,
                            posts: &posts,
                        },
                    )
                })
                .await
        }));
    }

    for task in tasks {
        assert!(task.await.unwrap().is_ok());
    }

    // Not "at most two renders". Eight tasks over two keys can all miss
    // before any of them inserts, and then all eight render — a thundering
    // herd. Coalescing concurrent misses onto one render is the cache's job,
    // not this crate's, and whether it does so is worth knowing before
    // relying on it. What is guaranteed here is only that every task got a
    // page and that at least one render happened.
    let misses = cache.misses.load(Ordering::Relaxed);
    assert!((1..=8).contains(&misses), "{misses} renders for 8 requests");

    // Once the burst settles, the keys are cached.
    let before = cache.misses.load(Ordering::Relaxed);
    let (user, posts) = fixture();
    cache
        .get_or_insert(
            "page-0",
            sync_miss(|| {
                render_owned(
                    &pool,
                    Page {
                        user: &user,
                        posts: &posts,
                    },
                )
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        cache.misses.load(Ordering::Relaxed),
        before,
        "should have hit"
    );
}

#[tokio::test]
async fn a_synchronous_render_fits_the_async_boundary() {
    // For a page with nothing to await, `sync_miss` supplies the future the
    // cache's signature wants without making the body async.
    let pool = BufferPool::new(4 * 1024, 4);
    let cache = Cache::default();
    let (user, posts) = fixture();

    let page = cache
        .get_or_insert(
            "static",
            sync_miss(|| {
                render_owned(
                    &pool,
                    Page {
                        user: &user,
                        posts: &posts,
                    },
                )
            }),
        )
        .await
        .unwrap();

    assert!(page.contains("Verified"));
}

#[tokio::test]
async fn a_failing_miss_caches_nothing() {
    let cache = Cache::default();
    let result: rsx::Result<Rendered> = cache
        .get_or_insert("bad", || async {
            Err(rsx::Error::custom("the database went away"))
        })
        .await;

    assert!(result.is_err());
    assert!(cache.entries.lock().unwrap().is_empty());
}
