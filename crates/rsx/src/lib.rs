//! Compile-time HTML templates for Rust.
//!
//! A `.rsx` file is Rust: `use` declarations, items, and functions. A function
//! returning `Element` has a markup body instead of a Rust one.
//!
//! ```text
//! use crate::models::User;
//!
//! pub fn Card(title: &str, count: usize) -> Element {
//!     <div class="card">
//!         <h2>{title}</h2>
//!         <span>{count}</span>
//!     </div>
//! }
//!
//! pub fn Page(user: &User) -> Element {
//!     <Card title={&user.name} count={user.posts.len()} />
//! }
//! ```
//!
//! Load them with the macro, which compiles the files during expansion:
//!
//! ```ignore
//! rsx::load!("src/views/card.rsx");
//! rsx::load_dir!("src/views");
//! ```
//!
//! Each component generates a writer function (`card`) and a composable value
//! (`Card`), so `Card { title: "Hi", count: 2 }.render_to_string()?` works, and
//! so does passing it to another component as children.

mod error;
mod escape;
mod template;
mod writer;

pub use error::{Error, Result};
pub use escape::{escape_into, is_clean};
pub use template::{from_fn, raw, DynTemplate, FromFn, Template};
pub use writer::{Attr, AttrValue, Writer};

#[cfg(feature = "macros")]
pub use rsx_macros::{load, load_dir};

/// Renders a template to a `String`.
pub fn render(template: impl Template) -> Result<String> {
    template.render_to_string()
}

/// Renders a template into an [`std::io::Write`], flushing in chunks.
pub fn render_to(template: impl Template, sink: &mut (dyn std::io::Write + Send)) -> Result {
    template.render_to(sink)
}
