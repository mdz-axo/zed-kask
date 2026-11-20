//! HTML page → text, for the keyless extractor (RawFetch).
//!
//! The page is read with html5ever's tokenizer (the WHATWG tokenization
//! algorithm: doctype, comments, character references, raw-text elements)
//! and its text is written as light Markdown: headings, paragraph breaks,
//! list items, table rows, preformatted blocks and link targets.
//!
//! No tree is built. html5ever's tree builder costs quadratic time on
//! nested block elements (measured 2026-11-12, release build: 16,000 nested
//! `<div>` 326 ms, 16,000 `<div>` side by side 7 ms), and nesting depth is
//! chosen by the page. The tokenizer keeps one tag's attributes in
//! a list it searches for each new attribute, so a single tag with very
//! many attributes is still quadratic; [`PARSE_BUDGET`] bounds that.
//!
//! This replaces a hand-written character scanner that returned an empty
//! string for any page beginning with a doctype (it read every `<!` as a
//! comment start), lost all text after an inline script containing `<`,
//! left character references undecoded and dropped link targets.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::{Rawtext, Rcdata, ScriptData};
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};

/// The text of an HTML page and its `<title>`.
#[derive(Debug)]
pub(crate) struct PageText {
    pub title: Option<String>,
    pub text: String,
}

/// The page could not be read within the parse budget.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ParseBudgetExceeded;

/// Time allowed for reading one page. A page of ordinary markup takes
/// milliseconds per megabyte; only markup built to be slow reaches this.
pub(crate) const PARSE_BUDGET: Duration = Duration::from_secs(2);

/// Input is handed to the tokenizer in pieces of this size, and the budget
/// is checked between pieces.
const CHUNK_BYTES: usize = 16 * 1024;

/// Elements whose content is never page text. `script`, `style` and the
/// other raw-text elements are handled by the tokenizer's raw-text states.
const NON_TEXT: &[&str] = &[
    "svg", "math", "template", "object", "canvas", "select", "datalist",
];

/// Raw-text elements whose text is not page text.
const RAW_NON_TEXT: &[&str] = &[
    "script", "style", "iframe", "noembed", "noframes", "textarea",
];

/// Page furniture left out when only the main content is wanted.
const FURNITURE: &[&str] = &["nav", "aside", "footer"];
const FURNITURE_ROLES: &[&str] = &["navigation", "complementary", "contentinfo"];

/// Elements with no end tag: they never open a skipped or tracked region.
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// Elements that start and end a paragraph-level block.
const BLOCKS: &[&str] = &[
    "p",
    "div",
    "section",
    "article",
    "main",
    "header",
    "footer",
    "aside",
    "nav",
    "blockquote",
    "dl",
    "table",
    "figure",
    "figcaption",
    "form",
    "fieldset",
    "address",
    "details",
    "summary",
    "hr",
];

/// Extract the text of `html` within [`PARSE_BUDGET`].
///
/// `base` resolves relative link targets; without it only absolute targets
/// are kept. With `main_content_only`, the first `<main>` (or `role="main"`)
/// element is the whole result when the page has one, and navigation,
/// sidebars and footers are left out.
pub(crate) fn page_text(
    html: &str,
    base: Option<&reqwest::Url>,
    main_content_only: bool,
) -> Result<PageText, ParseBudgetExceeded> {
    page_text_within(html, base, main_content_only, PARSE_BUDGET)
}

fn page_text_within(
    html: &str,
    base: Option<&reqwest::Url>,
    main_content_only: bool,
    budget: Duration,
) -> Result<PageText, ParseBudgetExceeded> {
    let started = Instant::now();
    let tokenizer = Tokenizer::new(
        Sink(RefCell::new(Writer::new(base, main_content_only))),
        TokenizerOpts::default(),
    );
    let queue = BufferQueue::default();
    let mut rest = html;
    while !rest.is_empty() {
        let (chunk, tail) = rest.split_at(rest.floor_char_boundary(CHUNK_BYTES.min(rest.len())));
        queue.push_back(StrTendril::from_slice(chunk));
        // The sink never suspends the tokenizer for a script, so `feed`
        // always consumes the queue.
        let _ = tokenizer.feed(&queue);
        rest = tail;
        if started.elapsed() > budget {
            return Err(ParseBudgetExceeded);
        }
    }
    tokenizer.end();
    let writer = tokenizer.sink.0.into_inner();
    Ok(PageText {
        title: writer
            .title
            .map(|title| title.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|title| !title.is_empty()),
        text: writer.out.trim().to_string(),
    })
}

struct Sink<'a>(RefCell<Writer<'a>>);

impl TokenSink for Sink<'_> {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        let mut writer = self.0.borrow_mut();
        match token {
            Token::CharacterTokens(text) => {
                writer.characters(&text);
                TokenSinkResult::Continue
            }
            Token::TagToken(tag) => writer.tag(&tag),
            _ => TokenSinkResult::Continue,
        }
    }
}

/// A region being left out: the element that opened it, and how many
/// elements of that name are open inside it.
struct Skip {
    name: String,
    depth: usize,
    /// SVG or MathML: `<x/>` closes itself there and raw-text rules do not
    /// apply.
    foreign: bool,
}

struct Writer<'a> {
    out: String,
    title: Option<String>,
    in_title: bool,
    /// Inside a raw-text element whose text is not page text.
    raw_non_text: bool,
    skip: Option<Skip>,
    /// The main-content element and how many elements of its name are open.
    main: Option<(String, usize)>,
    /// The main-content element has closed: nothing after it is wanted.
    done: bool,
    /// Whitespace seen since the last written character; written as one
    /// space only if more text follows on the same line.
    pending_space: bool,
    pre_depth: usize,
    /// A newline directly after `<pre>` is not part of its text.
    pre_just_opened: bool,
    code_depth: usize,
    /// Open lists, innermost last: `None` for `<ul>`, the next item number
    /// for `<ol>`.
    lists: Vec<Option<u32>>,
    /// The open link: where its text starts in `out`, and its target.
    link: Option<(usize, Option<String>)>,
    row_has_cell: bool,
    base: Option<&'a reqwest::Url>,
    main_content_only: bool,
}

fn attribute(tag: &Tag, wanted: &str) -> Option<String> {
    tag.attrs
        .iter()
        .find(|attr| &*attr.name.local == wanted)
        .map(|attr| attr.value.to_string())
}

fn heading_level(name: &str) -> Option<usize> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

impl<'a> Writer<'a> {
    fn new(base: Option<&'a reqwest::Url>, main_content_only: bool) -> Self {
        Self {
            out: String::new(),
            title: None,
            in_title: false,
            raw_non_text: false,
            skip: None,
            main: None,
            done: false,
            pending_space: false,
            pre_depth: 0,
            pre_just_opened: false,
            code_depth: 0,
            lists: Vec::new(),
            link: None,
            row_has_cell: false,
            base,
            main_content_only,
        }
    }

    fn tag(&mut self, tag: &Tag) -> TokenSinkResult<()> {
        let name: &str = &tag.name;
        let start = tag.kind == TagKind::StartTag;
        let in_foreign = self.skip.as_ref().is_some_and(|skip| skip.foreign);
        // What the tokenizer must do next. A tree builder gives these
        // instructions; with no tree, this sink does.
        let next = match name {
            _ if !start || in_foreign => TokenSinkResult::Continue,
            "script" => TokenSinkResult::RawData(ScriptData),
            "style" | "iframe" | "noembed" | "noframes" | "xmp" => {
                TokenSinkResult::RawData(Rawtext)
            }
            "title" | "textarea" => TokenSinkResult::RawData(Rcdata),
            "plaintext" => TokenSinkResult::Plaintext,
            _ => TokenSinkResult::Continue,
        };
        if self.done {
            return next;
        }
        if let Some(skip) = &mut self.skip {
            if name == skip.name {
                if !start {
                    skip.depth -= 1;
                    if skip.depth == 0 {
                        self.skip = None;
                    }
                } else if !(skip.foreign && tag.self_closing) {
                    skip.depth += 1;
                }
            }
            return next;
        }
        if start {
            self.enter(name, tag);
        } else {
            self.exit(name);
        }
        next
    }

    fn characters(&mut self, text: &str) {
        if self.done || self.skip.is_some() || self.raw_non_text {
            return;
        }
        if self.in_title {
            if let Some(title) = &mut self.title {
                title.push_str(text);
            }
            return;
        }
        if self.pre_depth > 0 {
            let text = match (self.pre_just_opened, text.strip_prefix('\n')) {
                (true, Some(rest)) => rest,
                _ => text,
            };
            self.pre_just_opened = false;
            self.out.push_str(text);
            return;
        }
        for ch in text.chars() {
            if ch.is_whitespace() {
                self.pending_space = true;
            } else {
                self.flush_space();
                self.out.push(ch);
            }
        }
    }

    fn flush_space(&mut self) {
        if self.pending_space && !self.out.is_empty() && !self.out.ends_with(['\n', ' ']) {
            self.out.push(' ');
        }
        self.pending_space = false;
    }

    fn line_break(&mut self) {
        self.pending_space = false;
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn block_break(&mut self) {
        self.line_break();
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn enter(&mut self, name: &str, tag: &Tag) {
        let role = attribute(tag, "role");
        let foreign = matches!(name, "svg" | "math");
        let furniture = self.main_content_only
            && (FURNITURE.contains(&name)
                || role
                    .as_deref()
                    .is_some_and(|role| FURNITURE_ROLES.contains(&role)));
        if (NON_TEXT.contains(&name) || furniture) && !VOID.contains(&name) {
            if !(foreign && tag.self_closing) {
                self.skip = Some(Skip {
                    name: name.to_string(),
                    depth: 1,
                    foreign,
                });
            }
            return;
        }
        if RAW_NON_TEXT.contains(&name) {
            self.raw_non_text = true;
            return;
        }
        if name == "title" {
            // The first title is the page's; a later one is not text.
            self.in_title = true;
            if self.title.is_none() {
                self.title = Some(String::new());
            } else {
                self.raw_non_text = true;
            }
            return;
        }
        if self.main_content_only && !VOID.contains(&name) {
            match &mut self.main {
                Some((main, depth)) if main.as_str() == name => *depth += 1,
                None if name == "main" || role.as_deref() == Some("main") => {
                    // The page has a main-content element: what came
                    // before it is not wanted.
                    self.out.clear();
                    self.pending_space = false;
                    self.pre_depth = 0;
                    self.code_depth = 0;
                    self.lists.clear();
                    self.link = None;
                    self.main = Some((name.to_string(), 1));
                }
                _ => {}
            }
        }
        if self.pre_depth > 0 {
            match name {
                "pre" | "xmp" => self.pre_depth += 1,
                "br" => self.out.push('\n'),
                _ => {}
            }
            return;
        }
        if let Some(level) = heading_level(name) {
            self.block_break();
            self.out.push_str(&"#".repeat(level));
            self.out.push(' ');
            return;
        }
        match name {
            "ul" => {
                self.block_break();
                self.lists.push(None);
            }
            "ol" => {
                self.block_break();
                self.lists.push(Some(1));
            }
            "li" => {
                self.line_break();
                let depth = self.lists.len().saturating_sub(1);
                self.out.push_str(&"  ".repeat(depth));
                match self.lists.last_mut() {
                    Some(Some(number)) => {
                        self.out.push_str(&format!("{number}. "));
                        *number += 1;
                    }
                    _ => self.out.push_str("- "),
                }
            }
            "br" | "dt" | "dd" => self.line_break(),
            "tr" => {
                self.line_break();
                self.row_has_cell = false;
            }
            "td" | "th" => {
                if self.row_has_cell {
                    self.pending_space = false;
                    self.out.push_str(" | ");
                }
                self.row_has_cell = true;
            }
            "pre" | "xmp" => {
                self.block_break();
                self.out.push_str("```\n");
                self.pre_depth = 1;
                self.pre_just_opened = true;
            }
            "code" => {
                self.flush_space();
                self.out.push('`');
                self.code_depth += 1;
            }
            "a" => {
                // A link cannot contain a link: a new one ends the open one.
                self.close_link();
                self.flush_space();
                let target = attribute(tag, "href").and_then(|href| self.link_target(&href));
                self.link = Some((self.out.len(), target));
            }
            _ if BLOCKS.contains(&name) => self.block_break(),
            _ => {}
        }
    }

    fn exit(&mut self, name: &str) {
        if RAW_NON_TEXT.contains(&name) {
            self.raw_non_text = false;
            return;
        }
        if name == "title" {
            self.in_title = false;
            self.raw_non_text = false;
            return;
        }
        if let Some((main, depth)) = &mut self.main
            && main.as_str() == name
        {
            *depth -= 1;
            if *depth == 0 {
                self.done = true;
                return;
            }
        }
        if self.pre_depth > 0 {
            if matches!(name, "pre" | "xmp") {
                self.pre_depth -= 1;
                if self.pre_depth == 0 {
                    if !self.out.ends_with('\n') {
                        self.out.push('\n');
                    }
                    self.out.push_str("```");
                    self.block_break();
                }
            }
            return;
        }
        if heading_level(name).is_some() {
            self.block_break();
            return;
        }
        match name {
            "ul" | "ol" => {
                self.lists.pop();
                self.block_break();
            }
            "li" | "tr" => self.line_break(),
            "code" if self.code_depth > 0 => {
                self.code_depth -= 1;
                self.pending_space = false;
                self.out.push('`');
            }
            "a" => self.close_link(),
            _ if BLOCKS.contains(&name) => self.block_break(),
            _ => {}
        }
    }

    fn close_link(&mut self) {
        if let Some((start, Some(target))) = self.link.take()
            && start <= self.out.len()
        {
            let text = self.out[start..].to_string();
            // A link that wraps blocks, or has no text, stays as its text.
            if !text.trim().is_empty() && !text.contains('\n') {
                self.out.truncate(start);
                self.out.push_str(&format!("[{}]({target})", text.trim()));
            }
        }
    }

    /// The absolute http(s) target of a link, or `None` for a fragment, a
    /// non-web scheme, or a relative target with no base to resolve it.
    fn link_target(&self, href: &str) -> Option<String> {
        let href = href.trim();
        if href.is_empty() || href.starts_with('#') {
            return None;
        }
        let url = match self.base {
            Some(base) => base.join(href).ok()?,
            None => reqwest::Url::parse(href).ok()?,
        };
        matches!(url.scheme(), "http" | "https").then(|| url.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(html: &str) -> String {
        page_text(html, None, false).expect("within budget").text
    }

    /// expect: a page that begins with a doctype yields its text. The
    /// scanner this replaces returned an empty string for it.
    #[test]
    fn a_doctype_page_yields_its_text() {
        let page = page_text(
            "<!doctype html><html><head><title>Example  Domain</title>\
             <style>body{color:red}</style></head>\
             <body><h1>Example Domain</h1><p>This domain is for use in examples.</p></body></html>",
            None,
            false,
        )
        .expect("within budget");
        assert_eq!(page.title.as_deref(), Some("Example Domain"));
        assert_eq!(
            page.text,
            "# Example Domain\n\nThis domain is for use in examples."
        );
    }

    /// expect: text after an inline script that contains `<` survives, and
    /// neither script nor style text is page text.
    #[test]
    fn text_after_a_script_containing_a_less_than_sign_survives() {
        assert_eq!(
            text(
                "<p>before</p><script>if (a < b) { document.write('<p>x</p>'); }</script>\
                 <style>p > a { color: red }</style><p>after</p>"
            ),
            "before\n\nafter"
        );
    }

    /// expect: character references are decoded; comments are not text.
    #[test]
    fn character_references_are_decoded_and_comments_dropped() {
        assert_eq!(
            text(
                "<p>Fish &amp; chips &lt;3 &mdash; caf&eacute; &#8220;x&#8221;<!-- hidden --></p>"
            ),
            "Fish & chips <3 — café “x”"
        );
    }

    /// expect: `<br/>` and `<br>` both break the line; runs of whitespace
    /// in the source collapse to one space.
    #[test]
    fn line_breaks_and_whitespace_follow_html_rules() {
        assert_eq!(text("<p>one<br/>two<br>three</p>"), "one\ntwo\nthree");
        assert_eq!(text("<p>a\n   b\t\tc</p>  <p> d </p>"), "a b c\n\nd");
    }

    /// expect: heading levels are kept, lists are itemised (numbered when
    /// ordered, indented when nested), with or without the optional end
    /// tags.
    #[test]
    fn headings_and_lists_keep_their_structure() {
        assert_eq!(
            text(
                "<h2>Two</h2><h4>Four</h4><ul><li>a</li><li>b<ol><li>x</li><li>y</li></ol></li></ul>"
            ),
            "## Two\n\n#### Four\n\n- a\n- b\n\n  1. x\n  2. y"
        );
        assert_eq!(
            text("<ul><li>a<li>b</ul><p>one<p>two"),
            "- a\n- b\n\none\n\ntwo"
        );
    }

    /// expect: a link keeps its target, resolved against the page URL; a
    /// fragment or a non-web scheme keeps only its text.
    #[test]
    fn links_keep_resolved_web_targets_only() {
        let base = reqwest::Url::parse("https://example.org/docs/page.html").expect("base");
        let page = page_text(
            "<p>See <a href=\"../about\">the about page</a>, <a href=\"#top\">top</a>, \
             <a href=\"mailto:a@example.org\">mail</a> and \
             <a href=\"https://other.example/x?a=1&amp;b=2\">another site</a>.</p>",
            Some(&base),
            false,
        )
        .expect("within budget");
        assert_eq!(
            page.text,
            "See [the about page](https://example.org/about), top, mail and \
             [another site](https://other.example/x?a=1&b=2)."
        );
        // No base: a relative target cannot be resolved and is not invented.
        assert_eq!(text("<a href=\"/x\">rel</a>"), "rel");
    }

    /// expect: preformatted text keeps its line structure inside a fence;
    /// table cells are separated on one line per row.
    #[test]
    fn preformatted_text_and_table_rows_keep_their_lines() {
        assert_eq!(
            text("<pre>\nfn main() {\n    <b>run</b>();\n}</pre><p>after</p>"),
            "```\nfn main() {\n    run();\n}\n```\n\nafter"
        );
        assert_eq!(
            text(
                "<table><tr><th>Name</th><th>Year</th></tr><tr><td>RRF<td>2009</table><p>after</p>"
            ),
            "Name | Year\nRRF | 2009\n\nafter"
        );
    }

    /// expect: with `main_content_only`, a `<main>` element is the whole
    /// result and navigation, sidebars and footers are left out; without it
    /// they stay.
    #[test]
    fn main_content_only_leaves_out_page_furniture() {
        let html = "<body><header>Site name</header><nav><a href=\"https://example.org/\">Home</a></nav>\
                    <main><h1>Title</h1><p>Body text.</p><aside>Related</aside></main>\
                    <footer>Copyright</footer></body>";
        assert_eq!(
            page_text(html, None, true).expect("within budget").text,
            "# Title\n\nBody text."
        );
        let whole = text(html);
        assert!(whole.contains("[Home](https://example.org/)"), "{whole}");
        assert!(whole.contains("Related"), "{whole}");
        assert!(whole.contains("Copyright"), "{whole}");

        // No <main>: the whole body, still without the furniture.
        assert_eq!(
            page_text(
                "<body><div role=\"navigation\"><div>Menu</div></div><p>Only text.</p></body>",
                None,
                true
            )
            .expect("within budget")
            .text,
            "Only text."
        );
        // role=main on a div that contains other divs ends at its own end tag.
        assert_eq!(
            page_text(
                "<div>before</div><div role=\"main\"><div>inner</div>text</div><div>after</div>",
                None,
                true
            )
            .expect("within budget")
            .text,
            "inner\n\ntext"
        );
    }

    /// expect: `<noscript>` content is page text for a fetcher that runs no
    /// script; SVG, form controls and `<template>` content are not. A
    /// self-closing element inside SVG does not swallow the page.
    #[test]
    fn noscript_is_text_and_non_text_elements_are_not() {
        assert_eq!(
            text(
                "<noscript><p>This page needs JavaScript.</p></noscript>\
                 <svg><title>icon</title><script/><path d=\"M0 0\"/></svg>\
                 <select><option>pick</option></select><textarea>draft</textarea>\
                 <template><p>stamp</p></template><p>after</p>"
            ),
            "This page needs JavaScript.\n\nafter"
        );
    }

    /// expect: nesting depth costs nothing extra: 100,000 nested blocks are
    /// read within the budget. A tree-building parser took seconds for a
    /// sixth of this.
    #[test]
    fn deep_nesting_is_read_within_the_budget() {
        let depth = 100_000;
        let html = format!("{}deep{}", "<div>".repeat(depth), "</div>".repeat(depth));
        assert_eq!(text(&html), "deep");
    }

    /// expect: the page is fed to the tokenizer in pieces, and a token
    /// that straddles two pieces reads the same as one that does not. Each
    /// probe is slid across the boundary one byte at a time.
    #[test]
    fn tokens_that_straddle_a_chunk_boundary_read_the_same() {
        for (probe, expected) in [
            ("<p>Fish &amp; chips</p>", "Fish & chips"),
            ("<p>a<!-- gone -->b</p>", "ab"),
            ("<script>x < y</script><p>kept</p>", "kept"),
            (
                "<p><a href=\"https://example.org/x\">link</a></p>",
                "[link](https://example.org/x)",
            ),
            ("<p>café — naïve</p>", "café — naïve"),
        ] {
            for shift in 0..=probe.len() {
                let padding = "x".repeat(CHUNK_BYTES - "<p></p>".len() - shift);
                let html = format!("<p>{padding}</p>{probe}");
                assert_eq!(
                    text(&html),
                    format!("{padding}\n\n{expected}"),
                    "probe {probe:?} at shift {shift}"
                );
            }
        }
    }

    /// expect: markup built to be slow is refused when the budget runs
    /// out, not parsed to the end. One tag with 200,000 distinct attributes
    /// makes the tokenizer search an ever longer list.
    #[test]
    fn markup_that_exhausts_the_budget_is_refused() {
        let mut html = String::from("<p>start</p><div");
        for index in 0..200_000 {
            html.push_str(&format!(" a{index}"));
        }
        html.push_str(">end</div>");
        assert_eq!(
            page_text_within(&html, None, false, Duration::from_millis(20)).unwrap_err(),
            ParseBudgetExceeded
        );
        // An ordinary page is nowhere near the budget.
        assert_eq!(text("<p>ordinary</p>"), "ordinary");
    }
}
