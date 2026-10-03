use std::io;

use crate::{Result, Writer};

/// A renderable piece of markup.
///
/// Every component in a `.rsx` file generates two things: a plain function that
/// writes into a [`Writer`], and a value implementing this trait that defers to
/// it. The function is the fast path; the value is what makes components
/// composable — it can be stored, passed to a layout as `children`, or wrapped.
///
/// `render` takes `self` by value so a component can own its parameters without
/// forcing `Copy` or `Clone` on them. For a heterogeneous collection, use
/// [`DynTemplate`], which is object safe.
pub trait Template {
    /// Writes this template's markup into `w`.
    fn render(self, w: &mut Writer<'_>) -> Result;

    /// Renders to a `String`.
    fn render_to_string(self) -> Result<String>
    where
        Self: Sized,
    {
        let mut w = Writer::new();
        self.render(&mut w)?;
        w.into_string()
    }

    /// Renders straight into an [`io::Write`], flushing in chunks.
    fn render_to(self, sink: &mut (dyn io::Write + Send)) -> Result
    where
        Self: Sized,
    {
        let mut w = Writer::streaming(sink);
        self.render(&mut w)?;
        w.finish()
    }
}

/// Renders nothing. This is what a component with no children receives.
impl Template for () {
    #[inline]
    fn render(self, _w: &mut Writer<'_>) -> Result {
        Ok(())
    }
}

/// Renders the inner template when present.
impl<T: Template> Template for Option<T> {
    #[inline]
    fn render(self, w: &mut Writer<'_>) -> Result {
        match self {
            Some(t) => t.render(w),
            None => Ok(()),
        }
    }
}

/// Renders each item in order.
impl<T: Template> Template for Vec<T> {
    fn render(self, w: &mut Writer<'_>) -> Result {
        for item in self {
            item.render(w)?;
        }
        Ok(())
    }
}

/// A template built from a closure. This is what inline children compile to.
pub struct FromFn<F>(F);

impl<F> Template for FromFn<F>
where
    F: FnOnce(&mut Writer<'_>) -> Result,
{
    #[inline]
    fn render(self, w: &mut Writer<'_>) -> Result {
        (self.0)(w)
    }
}

/// Wraps a closure as a [`Template`].
///
/// Generated code uses this for the children block of a component call, so the
/// children stay a zero-sized closure rather than an allocated node tree.
#[inline]
pub fn from_fn<F>(f: F) -> FromFn<F>
where
    F: FnOnce(&mut Writer<'_>) -> Result,
{
    FromFn(f)
}

/// Writes a pre-escaped HTML string verbatim.
///
/// The caller asserts the contents are safe HTML.
pub fn raw(html: impl AsRef<str>) -> impl Template {
    from_fn(move |w| w.raw(html.as_ref()))
}

/// The object-safe form of [`Template`], for `Box<dyn DynTemplate>`.
///
/// Implemented automatically for every `Template`, so a `Vec` of unrelated
/// components can be built with `Box::new(Card { .. }) as Box<dyn DynTemplate>`.
pub trait DynTemplate {
    /// Writes this template's markup into `w`.
    fn dyn_render(self: Box<Self>, w: &mut Writer<'_>) -> Result;
}

impl<T: Template> DynTemplate for T {
    #[inline]
    fn dyn_render(self: Box<Self>, w: &mut Writer<'_>) -> Result {
        (*self).render(w)
    }
}

impl Template for Box<dyn DynTemplate + '_> {
    #[inline]
    fn render(self, w: &mut Writer<'_>) -> Result {
        self.dyn_render(w)
    }
}
