//! HTML emission for the product web surface.
//!
//! The frozen product composes its documents with an element builder over a `TextWriter`.
//! The port keeps the same document shape but emits it directly: every element is closed
//! explicitly, and every dynamic value passes through [`escape`], so no asset name, path or
//! message table entry can inject markup.

use std::borrow::Cow;
use std::fmt::Write as _;

/// Escape a value for use as element content or inside a double-quoted attribute.
///
/// The input is returned unchanged when it contains nothing to escape, which keeps the
/// common case allocation-free.
pub fn escape(text: &str) -> Cow<'_, str> {
    if !text
        .bytes()
        .any(|byte| matches!(byte, b'&' | b'<' | b'>' | b'"' | b'\''))
    {
        return Cow::Borrowed(text);
    }
    let mut escaped = String::with_capacity(text.len() + 16);
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    Cow::Owned(escaped)
}

/// An HTML document under construction.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Html {
    text: String,
}

impl Html {
    /// An empty document.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append markup verbatim. Callers use this only for text built from [`escape`].
    pub fn raw(&mut self, markup: &str) -> &mut Self {
        self.text.push_str(markup);
        self
    }

    /// Append escaped text content.
    pub fn text(&mut self, content: &str) -> &mut Self {
        self.text.push_str(&escape(content));
        self
    }

    /// Open an element with attributes.
    pub fn open(&mut self, tag: &str, attributes: &[(&str, &str)]) -> &mut Self {
        let _ = write!(self.text, "<{tag}");
        self.write_attributes(attributes);
        self.text.push('>');
        self
    }

    /// Close an element.
    pub fn close(&mut self, tag: &str) -> &mut Self {
        let _ = write!(self.text, "</{tag}>");
        self
    }

    /// Write an element with escaped text content.
    pub fn element(&mut self, tag: &str, attributes: &[(&str, &str)], content: &str) -> &mut Self {
        self.open(tag, attributes);
        self.text(content);
        self.close(tag);
        self
    }

    /// Write a void element.
    pub fn void(&mut self, tag: &str, attributes: &[(&str, &str)]) -> &mut Self {
        let _ = write!(self.text, "<{tag}");
        self.write_attributes(attributes);
        self.text.push('>');
        self
    }

    fn write_attributes(&mut self, attributes: &[(&str, &str)]) {
        for (name, value) in attributes {
            let _ = write!(self.text, " {name}=\"{}\"", escape(value));
        }
    }

    /// The document text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Consume the builder and return the document text.
    pub fn into_string(self) -> String {
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::{escape, Html};

    #[test]
    fn escape_covers_every_markup_significant_character() {
        assert_eq!(escape("plain"), "plain");
        assert_eq!(escape("a&b"), "a&amp;b");
        assert_eq!(escape("<script>"), "&lt;script&gt;");
        assert_eq!(escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(escape("it's"), "it&#39;s");
        assert_eq!(
            escape("<a href=\"x\">&</a>"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;"
        );
    }

    #[test]
    fn builder_escapes_content_and_attributes() {
        let mut html = Html::new();
        html.open("div", &[("class", "a\"b")])
            .text("<b>plain</b>")
            .close("div");
        assert_eq!(
            html.as_str(),
            "<div class=\"a&quot;b\">&lt;b&gt;plain&lt;/b&gt;</div>"
        );
    }

    #[test]
    fn builder_nests_elements_and_voids() {
        let mut html = Html::new();
        html.open("ul", &[]);
        html.element("li", &[], "one");
        html.void("br", &[]);
        html.close("ul");
        assert_eq!(html.into_string(), "<ul><li>one</li><br></ul>");
    }
}
