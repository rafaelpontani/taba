//! A tiny writer for the XML of the AWS Query protocol (doc 39): elements with escaped text, in the order they are
//! written. The shape of each response is decided by the service module; this only builds the text.

/// Escapes the five characters XML reserves. Characters that XML 1.0 cannot carry at all are dropped.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// The text being written.
#[derive(Default)]
pub struct Xml {
    text: String,
}

impl Xml {
    pub fn new() -> Self {
        Self::default()
    }

    /// `<name xmlns="...">` (the namespace is optional).
    pub fn open(&mut self, name: &str, namespace: Option<&str>) -> &mut Self {
        self.text.push('<');
        self.text.push_str(name);
        if let Some(ns) = namespace {
            self.text.push_str(" xmlns=\"");
            self.text.push_str(ns);
            self.text.push('"');
        }
        self.text.push('>');
        self
    }

    pub fn close(&mut self, name: &str) -> &mut Self {
        self.text.push_str("</");
        self.text.push_str(name);
        self.text.push('>');
        self
    }

    /// `<name>text</name>`, with the text escaped.
    pub fn leaf(&mut self, name: &str, value: &str) -> &mut Self {
        self.open(name, None);
        self.text.push_str(&escape(value));
        self.close(name)
    }

    /// Text between elements, escaped.
    pub fn text(&mut self, value: &str) -> &mut Self {
        self.text.push_str(&escape(value));
        self
    }

    pub fn finish(self) -> String {
        format!("<?xml version=\"1.0\"?>{}", self.text)
    }
}

#[cfg(test)]
mod tests;
