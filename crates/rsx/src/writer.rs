use core::fmt::{self, Display, Write as _};
use std::io;

use crate::escape::escape_into;
use crate::{Error, Result};

/// Default number of bytes buffered before a streaming writer flushes.
const DEFAULT_CHUNK: usize = 8 * 1024;

/// The sink every generated template writes into.
///
/// A `Writer` always appends to an owned byte buffer first. That keeps the hot
/// path — `raw` and `text` — free of virtual dispatch: writing a static HTML
/// fragment is a `memcpy` into a `Vec<u8>` and nothing more. When the writer
/// was built with a sink, the buffer is handed to it once it grows past
/// `chunk`, so a large page streams out instead of being held whole in memory.
pub struct Writer<'a> {
    buf: Vec<u8>,
    sink: Option<&'a mut (dyn io::Write + 'a)>,
    chunk: usize,
}

impl Default for Writer<'static> {
    fn default() -> Self {
        Writer::new()
    }
}

impl Writer<'static> {
    /// Creates a writer that buffers the whole document in memory.
    pub fn new() -> Self {
        Writer::with_capacity(1024)
    }

    /// Creates a buffering writer with a preallocated buffer.
    pub fn with_capacity(capacity: usize) -> Self {
        Writer {
            buf: Vec::with_capacity(capacity),
            sink: None,
            chunk: usize::MAX,
        }
    }
}

impl<'a> Writer<'a> {
    /// Creates a writer that streams into `sink` in chunks.
    pub fn streaming(sink: &'a mut (dyn io::Write + 'a)) -> Self {
        Writer {
            buf: Vec::with_capacity(DEFAULT_CHUNK),
            sink: Some(sink),
            chunk: DEFAULT_CHUNK,
        }
    }

    /// Sets how many bytes accumulate before the sink is written to.
    pub fn with_chunk_size(mut self, chunk: usize) -> Self {
        if self.sink.is_some() {
            self.chunk = chunk.max(1);
        }
        self
    }

    /// Writes a pre-escaped HTML fragment verbatim.
    ///
    /// Generated code calls this with string literals the compiler produced, so
    /// the contents are trusted by construction.
    #[inline]
    pub fn raw(&mut self, s: &str) -> Result {
        self.buf.extend_from_slice(s.as_bytes());
        self.maybe_flush()
    }

    /// Writes a single pre-escaped byte.
    #[inline]
    pub fn raw_byte(&mut self, b: u8) -> Result {
        self.buf.push(b);
        self.maybe_flush()
    }

    /// Writes a value's `Display` output, HTML-escaped.
    ///
    /// This backs `{ expr }`. Formatting goes straight into the buffer through
    /// an escaping adapter, so no intermediate `String` is allocated — an
    /// integer or a `&str` costs exactly one pass.
    #[inline]
    pub fn text(&mut self, value: impl Display) -> Result {
        // `Display::fmt` only fails if the sink fails, and ours cannot.
        let _ = write!(Escaped(&mut self.buf), "{value}");
        self.maybe_flush()
    }

    /// Writes a value's `Display` output *without* escaping.
    ///
    /// This backs `{! expr }`. The caller is asserting the value is already
    /// safe HTML; passing user input here is an XSS vulnerability.
    #[inline]
    pub fn raw_text(&mut self, value: impl Display) -> Result {
        let _ = write!(Unescaped(&mut self.buf), "{value}");
        self.maybe_flush()
    }

    /// Writes an attribute value, HTML-escaped. Backs `name={ expr }`.
    #[inline]
    pub fn attr(&mut self, value: impl Display) -> Result {
        self.text(value)
    }

    /// Writes ` name="value"` for an attribute whose value is an expression.
    #[inline]
    pub fn named_attr(&mut self, name: &str, value: impl Display) -> Result {
        self.raw_byte(b' ')?;
        self.raw(name)?;
        self.raw("=\"")?;
        self.attr(value)?;
        self.raw_byte(b'"')
    }

    /// Writes ` name` when `present` is true, nothing otherwise.
    ///
    /// Backs the `name?={ expr }` conditional-attribute form.
    #[inline]
    pub fn bool_attr(&mut self, name: &str, present: bool) -> Result {
        if present {
            self.raw_byte(b' ')?;
            self.raw(name)?;
        }
        Ok(())
    }

    /// Writes a collection of attributes onto the element being opened.
    ///
    /// Backs `{ ...expr }`. Accepts anything iterable as key/value pairs — a
    /// `HashMap`, a `BTreeMap`, a slice of tuples — where the value decides
    /// through [`AttrValue`] whether it renders as `k="v"`, as a bare `k`, or
    /// is skipped entirely.
    pub fn spread<I, K, V>(&mut self, attrs: I) -> Result
    where
        I: IntoIterator<Item = (K, V)>,
        K: Display,
        V: AttrValue,
    {
        for (name, value) in attrs {
            match value.to_attr() {
                Attr::Skip => {}
                Attr::Bare => {
                    self.raw_byte(b' ')?;
                    self.text(&name)?;
                }
                Attr::Str(v) => {
                    self.raw_byte(b' ')?;
                    self.text(&name)?;
                    self.raw("=\"")?;
                    self.attr(v)?;
                    self.raw_byte(b'"')?;
                }
                Attr::Value(v) => {
                    self.raw_byte(b' ')?;
                    self.text(&name)?;
                    self.raw("=\"")?;
                    self.attr(v)?;
                    self.raw_byte(b'"')?;
                }
            }
        }
        Ok(())
    }

    /// Bytes buffered but not yet handed to the sink.
    pub fn buffered(&self) -> &[u8] {
        &self.buf
    }

    /// Pushes everything buffered to the sink, if there is one.
    pub fn flush(&mut self) -> Result {
        if let Some(sink) = self.sink.as_mut() {
            if !self.buf.is_empty() {
                sink.write_all(&self.buf)?;
                self.buf.clear();
            }
            sink.flush()?;
        }
        Ok(())
    }

    /// Flushes and consumes the writer. Use this with [`Writer::streaming`].
    pub fn finish(mut self) -> Result {
        self.flush()
    }

    /// Consumes a buffering writer and returns the rendered bytes.
    ///
    /// Returns an error for a streaming writer, whose output has already left
    /// through the sink.
    pub fn into_bytes(self) -> Result<Vec<u8>> {
        if self.sink.is_some() {
            return Err(Error::custom(
                "Writer::into_bytes called on a streaming writer; use finish() instead",
            ));
        }
        Ok(self.buf)
    }

    /// Consumes a buffering writer and returns the rendered document.
    pub fn into_string(self) -> Result<String> {
        let bytes = self.into_bytes()?;
        // Every write path appends either a `&str` or an ASCII entity, so the
        // buffer is UTF-8 by construction.
        Ok(unsafe { String::from_utf8_unchecked(bytes) })
    }

    #[inline]
    fn maybe_flush(&mut self) -> Result {
        if self.buf.len() >= self.chunk {
            self.flush()?;
        }
        Ok(())
    }
}

impl io::Write for Writer<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        self.maybe_flush()?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Writer::flush(self).map_err(io::Error::from)
    }
}

/// A `fmt::Write` adapter that HTML-escapes as it formats.
struct Escaped<'b>(&'b mut Vec<u8>);

impl fmt::Write for Escaped<'_> {
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        escape_into(self.0, s);
        Ok(())
    }
}

/// A `fmt::Write` adapter that appends without escaping.
struct Unescaped<'b>(&'b mut Vec<u8>);

impl fmt::Write for Unescaped<'_> {
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

/// How a spread attribute's value renders.
pub enum Attr<'a> {
    /// Render the name alone, as in `disabled`.
    Bare,
    /// Render `name="value"` from a string, the common case.
    Str(&'a str),
    /// Render `name="value"` from any displayable value.
    Value(&'a dyn Display),
    /// Omit the attribute.
    Skip,
}

/// A value that can appear in a spread attribute collection.
pub trait AttrValue {
    /// Decides how this value renders.
    fn to_attr(&self) -> Attr<'_>;
}

impl AttrValue for bool {
    #[inline]
    fn to_attr(&self) -> Attr<'_> {
        if *self {
            Attr::Bare
        } else {
            Attr::Skip
        }
    }
}

impl<T: AttrValue> AttrValue for Option<T> {
    #[inline]
    fn to_attr(&self) -> Attr<'_> {
        match self {
            Some(v) => v.to_attr(),
            None => Attr::Skip,
        }
    }
}

impl<T: AttrValue + ?Sized> AttrValue for &T {
    #[inline]
    fn to_attr(&self) -> Attr<'_> {
        (**self).to_attr()
    }
}

/// Implements [`AttrValue`] as a plain `name="value"` for `Display` types.
macro_rules! attr_value_via_display {
    ($($ty:ty),* $(,)?) => {
        $(impl AttrValue for $ty {
            #[inline]
            fn to_attr(&self) -> Attr<'_> {
                Attr::Value(self)
            }
        })*
    };
}

/// `str` is unsized, so it cannot coerce to `&dyn Display`; it gets the
/// dedicated `Attr::Str` variant instead, which is also the faster path.
impl AttrValue for str {
    #[inline]
    fn to_attr(&self) -> Attr<'_> {
        Attr::Str(self)
    }
}

impl AttrValue for String {
    #[inline]
    fn to_attr(&self) -> Attr<'_> {
        Attr::Str(self)
    }
}

impl AttrValue for std::borrow::Cow<'_, str> {
    #[inline]
    fn to_attr(&self) -> Attr<'_> {
        Attr::Str(self)
    }
}

attr_value_via_display!(
    char, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64,
);
