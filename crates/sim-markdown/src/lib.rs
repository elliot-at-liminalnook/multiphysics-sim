//! Renderer-independent Markdown for inspector text and annotation discussions.
//! Links remain typed data. HTML is inert text; no browser or script execution.
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use serde::Serialize;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
    pub link: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Paragraph,
    Heading(u8),
    Code(String),
    Quote,
    Rule,
    /// GitHub-style table; the cells are in `Block::table`.
    Table,
    /// An image (figure); source and caption are in `Block::image`.
    Image,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Block {
    pub kind: Kind,
    pub indent: usize,
    pub spans: Vec<Span>,
    /// Present for `Kind::Table`.
    pub table: Option<Table>,
    /// Present for `Kind::Image`.
    pub image: Option<Image>,
}
/// `![alt](src "title")`. Images are always their own block (a figure).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Image {
    pub src: String,
    pub alt: String,
    pub title: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    None,
    Left,
    Center,
    Right,
}
/// Rows of rich-text cells. Rows may have fewer cells than the header.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub align: Vec<Align>,
    pub head: Vec<Vec<Span>>,
    pub rows: Vec<Vec<Vec<Span>>>,
}
impl Table {
    /// A table of plain text (for hosts that build tables from data).
    pub fn plain(head: &[String], rows: &[Vec<String>], align: &[Align]) -> Self {
        let cell = |t: &String| if t.is_empty() { vec![] } else { vec![Span { text: t.clone(), style: Style::default() }] };
        Self { align: align.to_vec(), head: head.iter().map(cell).collect(), rows: rows.iter().map(|r| r.iter().map(cell).collect()).collect() }
    }
    pub fn columns(&self) -> usize {
        self.rows.iter().map(Vec::len).chain([self.head.len()]).max().unwrap_or(0)
    }
    fn plain_text(&self) -> String {
        let row = |cells: &Vec<Vec<Span>>| cells.iter().map(|c| c.iter().map(|s| s.text.as_str()).collect::<String>()).collect::<Vec<_>>().join(" | ");
        std::iter::once(row(&self.head)).chain(self.rows.iter().map(row)).collect::<Vec<_>>().join("\n")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub label: String,
    pub target: String,
}
#[derive(Clone, Debug, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub links: Vec<Link>,
}
impl Document {
    pub fn plain(&self) -> String {
        self.blocks
            .iter()
            .map(|b| match (&b.table, &b.image) {
                (Some(t), _) => t.plain_text(),
                (_, Some(i)) => if i.title.is_empty() { i.alt.clone() } else { i.title.clone() },
                _ => b.spans.iter().map(|s| s.text.as_str()).collect::<String>(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
fn flush(block: &mut Block, out: &mut Document) {
    if !block.spans.is_empty() || block.kind == Kind::Rule || block.table.is_some() || block.image.is_some() {
        out.blocks.push(std::mem::take(block));
    }
}
fn text(block: &mut Block, s: &str, style: &Style) {
    if let Some(last) = block.spans.last_mut().filter(|v| v.style == *style) {
        last.text.push_str(s);
    } else {
        block.spans.push(Span {
            text: s.into(),
            style: style.clone(),
        });
    }
}
pub fn parse(source: &str) -> Document {
    let mut out = Document::default();
    let mut block = Block::default();
    let mut style = Style::default();
    let mut styles = vec![];
    let mut lists: Vec<Option<u64>> = vec![];
    let mut quote = 0;
    let mut code = false;
    let mut link_label = String::new();
    // Inside a table: the table so far, the row being read and the cell's spans.
    let mut table: Option<Table> = None;
    let mut row: Vec<Vec<Span>> = Vec::new();
    let mut cell: Option<Block> = None;
    // Inside an image: its alt text is collected here, not in the paragraph.
    let mut image: Option<Image> = None;
    for event in Parser::new_ext(source, Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES) {
        match event {
            Event::Start(Tag::Paragraph) => {
                if block.spans.is_empty() {
                    block.kind = if quote > 0 {
                        Kind::Quote
                    } else {
                        Kind::Paragraph
                    };
                }
                block.indent = lists.len().saturating_sub(1);
            }
            Event::End(TagEnd::Paragraph) => flush(&mut block, &mut out),
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut block, &mut out);
                block.kind = Kind::Heading(level as u8);
            }
            Event::End(TagEnd::Heading(_)) => flush(&mut block, &mut out),
            Event::Start(Tag::CodeBlock(kind)) => {
                flush(&mut block, &mut out);
                block.kind = Kind::Code(match kind {
                    CodeBlockKind::Fenced(s) => s.into_string(),
                    _ => String::new(),
                });
                code = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                flush(&mut block, &mut out);
                code = false;
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush(&mut block, &mut out);
                quote += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(&mut block, &mut out);
                quote -= 1;
            }
            Event::Start(Tag::List(first)) => {
                flush(&mut block, &mut out);
                lists.push(first);
            }
            Event::End(TagEnd::List(_)) => {
                flush(&mut block, &mut out);
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                flush(&mut block, &mut out);
                block.indent = lists.len().saturating_sub(1);
                let prefix = match lists.last_mut() {
                    Some(Some(n)) => {
                        let p = format!("{n}. ");
                        *n += 1;
                        p
                    }
                    _ => "• ".into(),
                };
                text(&mut block, &prefix, &Style::default());
            }
            Event::End(TagEnd::Item) => flush(&mut block, &mut out),
            Event::Start(Tag::Strong) => {
                styles.push(style.clone());
                style.strong = true;
            }
            Event::Start(Tag::Emphasis) => {
                styles.push(style.clone());
                style.emphasis = true;
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                styles.push(style.clone());
                style.link = Some(dest_url.into_string());
                link_label.clear();
            }
            Event::End(TagEnd::Link) => {
                if let Some(target) = &style.link {
                    let l = Link {
                        label: link_label.clone(),
                        target: target.clone(),
                    };
                    if !out.links.contains(&l) {
                        out.links.push(l);
                    }
                }
                style = styles.pop().unwrap_or_default();
            }
            Event::End(TagEnd::Strong | TagEnd::Emphasis) => {
                style = styles.pop().unwrap_or_default()
            }
            Event::Start(Tag::Image { dest_url, title, .. }) => {
                let kind = block.kind.clone();
                let indent = block.indent;
                flush(&mut block, &mut out);
                block.kind = kind;
                block.indent = indent;
                image = Some(Image { src: dest_url.into_string(), alt: String::new(), title: title.into_string() });
            }
            Event::End(TagEnd::Image) => {
                let kind = block.kind.clone();
                let indent = block.indent;
                let mut figure = Block { kind: Kind::Image, indent, image: image.take(), ..Default::default() };
                flush(&mut figure, &mut out);
                block.kind = kind;
            }
            Event::Text(s) if image.is_some() => image.as_mut().unwrap().alt.push_str(&s),
            Event::Text(s) => {
                if style.link.is_some() {
                    link_label.push_str(&s);
                }
                let mut st = style.clone();
                st.code |= code;
                text(cell.as_mut().unwrap_or(&mut block), &s, &st);
            }
            Event::Code(s) => {
                let mut st = style.clone();
                st.code = true;
                if style.link.is_some() {
                    link_label.push_str(&s);
                }
                text(cell.as_mut().unwrap_or(&mut block), &s, &st);
            }
            Event::SoftBreak => text(cell.as_mut().unwrap_or(&mut block), " ", &style),
            Event::HardBreak => text(cell.as_mut().unwrap_or(&mut block), "\n", &style),
            Event::Rule => {
                flush(&mut block, &mut out);
                block.kind = Kind::Rule;
                flush(&mut block, &mut out);
            }
            Event::TaskListMarker(done) => {
                text(&mut block, if done { "[x] " } else { "[ ] " }, &style)
            }
            Event::Start(Tag::Table(align)) => {
                flush(&mut block, &mut out);
                table = Some(Table {
                    align: align.iter().map(|a| match a {
                        pulldown_cmark::Alignment::None => Align::None,
                        pulldown_cmark::Alignment::Left => Align::Left,
                        pulldown_cmark::Alignment::Center => Align::Center,
                        pulldown_cmark::Alignment::Right => Align::Right,
                    }).collect(),
                    ..Default::default()
                });
            }
            Event::Start(Tag::TableHead | Tag::TableRow) => row.clear(),
            Event::Start(Tag::TableCell) => cell = Some(Block::default()),
            Event::End(TagEnd::TableCell) => row.push(cell.take().map(|c| c.spans).unwrap_or_default()),
            Event::End(TagEnd::TableHead) => {
                if let Some(t) = table.as_mut() {
                    t.head = std::mem::take(&mut row);
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(t) = table.as_mut() {
                    t.rows.push(std::mem::take(&mut row));
                }
            }
            Event::End(TagEnd::Table) => {
                block.kind = Kind::Table;
                block.indent = lists.len().saturating_sub(1);
                block.table = table.take();
                flush(&mut block, &mut out);
            }
            Event::Html(s) | Event::InlineHtml(s) => {
                let mut st = style.clone();
                st.code = true;
                text(&mut block, &s, &st);
            }
            _ => {}
        }
    }
    flush(&mut block, &mut out);
    out
}

#[cfg(test)]
mod table_tests {
    use super::*;
    #[test]
    fn images_become_their_own_figure_blocks() {
        let doc = parse("Before ![The torque line](torque-speed.svg \"Torque falls linearly\") after.");
        let kinds: Vec<&Kind> = doc.blocks.iter().map(|b| &b.kind).collect();
        assert_eq!(kinds, vec![&Kind::Paragraph, &Kind::Image, &Kind::Paragraph]);
        let i = doc.blocks[1].image.as_ref().unwrap();
        assert_eq!((i.src.as_str(), i.alt.as_str(), i.title.as_str()), ("torque-speed.svg", "The torque line", "Torque falls linearly"));
        assert_eq!(doc.blocks[0].spans[0].text.trim(), "Before");
    }
    #[test]
    fn tables_keep_cells_alignment_and_inline_style() {
        let doc = parse("Before.\n\n| Gearbox | Speed |\n|:--|--:|\n| **Worm** | 26.7 |\n| `spur` | 30.3 |\n\nAfter.");
        let kinds: Vec<&Kind> = doc.blocks.iter().map(|b| &b.kind).collect();
        assert_eq!(kinds, vec![&Kind::Paragraph, &Kind::Table, &Kind::Paragraph]);
        let t = doc.blocks[1].table.as_ref().unwrap();
        assert_eq!(t.align, vec![Align::Left, Align::Right]);
        assert_eq!(t.head[1][0].text, "Speed");
        assert_eq!(t.rows.len(), 2);
        assert!(t.rows[0][0][0].style.strong && t.rows[1][0][0].style.code);
        assert!(doc.plain().contains("Worm | 26.7"));
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceLine {
    pub number: usize,
    pub text: String,
    pub focused: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct Source {
    pub path: String,
    pub line: usize,
    pub lines: Vec<SourceLine>,
}
/// Local paths only, with optional :line, :line:column or #Lline suffix.
pub fn source_location(target: &str) -> Result<(String, usize), String> {
    let target = target.strip_prefix("file://").unwrap_or(target);
    if target.contains("://")
        || target.starts_with("javascript:")
        || target.starts_with("data:")
        || target.starts_with("part:")
        || target.starts_with("group:")
    {
        return Err("not a local source reference".into());
    }
    let mut path = target.to_string();
    let mut line = 1;
    if let Some((p, l)) = path.rsplit_once("#L") {
        line = l
            .split('-')
            .next()
            .unwrap_or(l)
            .parse()
            .map_err(|_| "invalid source line")?;
        path = p.into();
    } else if let Some((p, l)) = path.rsplit_once(':') {
        if let Ok(n) = l.parse::<usize>() {
            line = n;
            let mut p = p.to_string();
            if let Some((file, l)) = p.rsplit_once(':') {
                if let Ok(n) = l.parse::<usize>() {
                    line = n;
                    p = file.into();
                }
            }
            path = p;
        }
    }
    if path.is_empty() || path.contains('\0') || line == 0 {
        return Err("invalid source reference".into());
    }
    Ok((path, line))
}
/// Call off the UI thread. Resolve symlinks and keep previews within the project.
pub fn read_source(root: &std::path::Path, target: &str) -> Result<Source, String> {
    use std::io::Read;
    let (path, line) = source_location(target)?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let candidate = root.join(path).canonicalize().map_err(|e| e.to_string())?;
    if !candidate.starts_with(&root) {
        return Err("source reference is outside this project".into());
    }
    let mut file = std::fs::File::open(&candidate).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("source reference is not a regular file".into());
    }
    let mut bytes = vec![];
    file.by_ref()
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("source preview is limited to 1 MiB".into());
    }
    let body = String::from_utf8(bytes).map_err(|_| "source file is not UTF-8 text")?;
    if line > body.lines().count().max(1) {
        return Err("source line is beyond the end of the file".into());
    }
    let lines = body
        .lines()
        .enumerate()
        .filter(|(i, _)| i + 1 >= line.saturating_sub(4) && i + 1 <= line.saturating_add(9))
        .map(|(i, s)| SourceLine {
            number: i + 1,
            text: s.chars().take(500).collect(),
            focused: i + 1 == line,
        })
        .collect();
    Ok(Source {
        path: candidate.strip_prefix(root).unwrap().display().to_string(),
        line,
        lines,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_citations_and_nested_styles_without_raw_destinations() {
        let d = parse(
            "The **CAD _model_** owns it, per [AGENTS.md:6](/Users/a/project/AGENTS.md:6). Use `mass`.\n\n- first\n- second\n\n```rust\nlet x = 1;\n```",
        );
        assert!(!d.plain().contains("/Users"));
        assert!(d.plain().contains("AGENTS.md:6"));
        assert!(
            d.blocks
                .iter()
                .flat_map(|b| &b.spans)
                .any(|s| s.style.strong && s.style.emphasis)
        );
        assert_eq!(d.links[0].target, "/Users/a/project/AGENTS.md:6");
        assert!(d.blocks.iter().any(|b| b.kind == Kind::Code("rust".into())));
        assert!(d.plain().contains("• second"));
    }
    #[test]
    fn html_is_inert_and_source_references_are_typed() {
        assert!(
            parse("<script>alert(1)</script>")
                .plain()
                .contains("<script>")
        );
        assert!(source_location("javascript:alert(1)").is_err());
        assert_eq!(source_location("a.rs:6:2").unwrap(), ("a.rs".into(), 6));
        assert_eq!(source_location("a.rs#L12").unwrap(), ("a.rs".into(), 12));
    }
    #[test]
    fn source_preview_is_bounded_to_project() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let s = read_source(root, "Cargo.toml:2").unwrap();
        assert!(s.lines.iter().any(|l| l.number == 2 && l.focused));
        assert!(read_source(root, "../sim-agent/Cargo.toml:2").is_err());
        assert!(read_source(root, "Cargo.toml:99999").is_err());
    }
}
