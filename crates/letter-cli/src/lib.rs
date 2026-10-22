//! Консольная утилита для отладки чтения и записи документов.
//!
//! - `letter-cli dump файл.docx` — печатает дерево модели документа;
//! - `letter-cli dump --debug файл.docx` — то же, но все поля подряд;
//! - `letter-cli roundtrip вход.docx выход.docx` — открывает, сохраняет,
//!   открывает сохранённое и сравнивает модели;
//! - `letter-cli pages [--embedded-fonts] файл.docx` — раскладывает
//!   документ по страницам и печатает строки каждой страницы;
//! - `letter-cli render [--embedded-fonts] файл.docx папка` — рисует
//!   страницы в картинки PNG (96 точек на дюйм, как Word при 100 %);
//! - `letter-cli compare [--embedded-fonts] [--tolerance пт] файл.docx
//!   эталон.pdf` — сверяет нашу вёрстку с PDF, сохранённым из Word;
//! - `letter-cli pdf-lines файл.pdf` — печатает строки текста PDF так, как
//!   их видит сверка.

pub mod compare;
pub mod pdf;

use std::fmt::Write as _;
use std::path::Path;

use letter_layout::{FontLibrary, Shaper, layout_document};
use letter_render::{GlyphCache, RgbaImage, SCREEN_PIXELS_PER_POINT, render_page};

use letter_model::{
    Alignment, Block, BreakKind, Document, FirstLineIndent, Inline, LineSpacing, Paragraph,
    ParagraphProps, Run, RunContent, RunProps, Section, Table,
};

/// Справка по командам.
pub const USAGE: &str = "\
Использование:
  letter-cli dump [--debug] файл.docx         напечатать дерево модели
  letter-cli roundtrip вход.docx выход.docx  открыть → сохранить → открыть и сравнить
  letter-cli pages [--embedded-fonts] файл.docx  разложить по страницам и напечатать строки
  letter-cli render [--embedded-fonts] файл.docx папка  нарисовать страницы в PNG
  letter-cli compare [--embedded-fonts] [--tolerance пт] файл.docx эталон.pdf
                                               сверить вёрстку с PDF из Word
  letter-cli pdf-lines файл.pdf               напечатать строки текста PDF";

/// Выполняет команду и возвращает текст для вывода. Ошибка — текст
/// для вывода в поток ошибок.
pub fn run(args: &[String]) -> Result<String, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["dump", path] => Ok(dump(&open(path)?)),
        ["dump", "--debug", path] => Ok(format!("{:#?}\n", open(path)?)),
        ["roundtrip", input, output] => roundtrip(Path::new(input), Path::new(output)),
        ["pages", path] => Ok(pages(&open(path)?, &FontLibrary::new())),
        ["pages", "--embedded-fonts", path] => {
            Ok(pages(&open(path)?, &FontLibrary::embedded_only()))
        }
        ["render", path, dir] => render(&open(path)?, &FontLibrary::new(), Path::new(dir)),
        ["render", "--embedded-fonts", path, dir] => {
            render(&open(path)?, &FontLibrary::embedded_only(), Path::new(dir))
        }
        ["compare", rest @ ..] => compare_command(rest),
        ["pdf-lines", path] => pdf_lines(Path::new(path)),
        _ => Err(USAGE.to_string()),
    }
}

fn compare_command(args: &[&str]) -> Result<String, String> {
    let mut library = None;
    let mut tolerance = compare::DEFAULT_TOLERANCE;
    let mut files = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match *arg {
            "--embedded-fonts" => library = Some(FontLibrary::embedded_only()),
            "--tolerance" => {
                tolerance = args
                    .next()
                    .and_then(|v| v.replace(',', ".").parse().ok())
                    .filter(|v: &f32| v.is_finite() && *v >= 0.0)
                    .ok_or_else(|| format!("--tolerance: нужно число пунктов\n{USAGE}"))?;
            }
            file => files.push(file),
        }
    }
    let [docx, reference] = files.as_slice() else {
        return Err(USAGE.to_string());
    };
    let library = library.unwrap_or_else(FontLibrary::new);
    let result = compare_files(Path::new(docx), Path::new(reference), &library, tolerance)?;
    Ok(result.report())
}

/// Строки текста PDF с положением, как их видит сверка.
pub fn pdf_lines(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let pages = pdf::read_pdf_lines(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let mm = |pt: f32| f64::from(pt) * 25.4 / 72.0;
    let mut out = String::new();
    for (i, page) in pages.iter().enumerate() {
        let _ = writeln!(
            out,
            "Страница {}: {:.1}×{:.1} мм, строк {}",
            i + 1,
            mm(page.width),
            mm(page.height),
            page.lines.len()
        );
        for line in &page.lines {
            let _ = writeln!(
                out,
                "  {:7.2} пт  x {:7.2}  {:?}",
                line.baseline, line.x, line.text
            );
        }
    }
    Ok(out)
}

/// Сверяет вёрстку документа `docx` с эталонным PDF `reference`.
pub fn compare_files(
    docx: &Path,
    reference: &Path,
    library: &FontLibrary,
    tolerance: f32,
) -> Result<compare::Comparison, String> {
    let doc = letter_docx::read_file(docx).map_err(|e| format!("{}: {e}", docx.display()))?;
    let bytes = std::fs::read(reference).map_err(|e| format!("{}: {e}", reference.display()))?;
    let pdf = pdf::read_pdf_lines(&bytes).map_err(|e| format!("{}: {e}", reference.display()))?;
    let layout = layout_document(&doc, &mut Shaper::new(), library);
    Ok(compare::compare(
        &compare::reference_lines(&pdf, &layout),
        pdf.len(),
        &compare::layout_lines(&layout),
        layout.pages.len(),
        tolerance,
    ))
}

/// Рисует страницы документа в `dir/page-1.png`, `dir/page-2.png`, …
pub fn render(doc: &Document, library: &FontLibrary, dir: &Path) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let layout = layout_document(doc, &mut Shaper::new(), library);
    let mut cache = GlyphCache::new();
    let mut out = String::new();
    for (i, page) in layout.pages.iter().enumerate() {
        let image = render_page(page, SCREEN_PIXELS_PER_POINT, library, &mut cache);
        let path = dir.join(format!("page-{}.png", i + 1));
        write_png(&image, &path).map_err(|e| format!("{}: {e}", path.display()))?;
        let _ = writeln!(out, "{} ({}×{})", path.display(), image.width, image.height);
    }
    Ok(out)
}

fn write_png(image: &RgbaImage, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&image.pixels)?;
    writer.finish()?;
    Ok(())
}

fn open(path: &str) -> Result<Document, String> {
    letter_docx::read_file(path).map_err(|e| format!("{path}: {e}"))
}

/// Открывает `input`, сохраняет в `output`, открывает сохранённое и
/// сравнивает модели.
pub fn roundtrip(input: &Path, output: &Path) -> Result<String, String> {
    let original =
        letter_docx::read_file(input).map_err(|e| format!("{}: {e}", input.display()))?;
    letter_docx::write_file(&original, output).map_err(|e| format!("{}: {e}", output.display()))?;
    let reopened =
        letter_docx::read_file(output).map_err(|e| format!("{}: {e}", output.display()))?;
    if original == reopened {
        Ok(format!(
            "Сохранено в {}. Модели до и после совпадают.\n",
            output.display()
        ))
    } else {
        let before = dump(&original);
        let after = dump(&reopened);
        let first_difference = before.lines().zip(after.lines()).position(|(a, b)| a != b);
        let detail = match first_difference {
            Some(i) => format!(
                "строка {}:\n  было:  {}\n  стало: {}",
                i + 1,
                before.lines().nth(i).unwrap_or_default(),
                after.lines().nth(i).unwrap_or_default()
            ),
            None => "отличаются поля, которые не видны в дереве (см. dump --debug)".to_string(),
        };
        Err(format!(
            "Сохранено в {}, но модель после повторного открытия отличается: {detail}",
            output.display()
        ))
    }
}

/// Страницы документа: размер, поля и строки с положением по вертикали.
pub fn pages(doc: &Document, library: &FontLibrary) -> String {
    let layout = layout_document(doc, &mut Shaper::new(), library);
    let mut out = String::new();
    let mm = |pt: f32| f64::from(pt) * 25.4 / 72.0;
    for (i, page) in layout.pages.iter().enumerate() {
        let _ = writeln!(
            out,
            "Страница {}: {:.1}×{:.1} мм, строк {}",
            i + 1,
            mm(page.width),
            mm(page.height),
            page.lines.len()
        );
        for line in &page.lines {
            let text = line.text.get(line.line.range.clone()).unwrap_or_default();
            let label = line
                .label
                .as_ref()
                .map(|l| format!("{} ", l.text))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "  {:7.2} пт  абзац {:<3} {label}{:?}",
                line.top,
                line.paragraph + 1,
                text.trim_end()
            );
        }
    }
    out
}

/// Дерево модели документа в читаемом виде.
pub fn dump(doc: &Document) -> String {
    let mut out = String::new();
    let core = &doc.properties.core;
    let _ = writeln!(
        out,
        "Документ: разделов {}, стилей {}",
        doc.sections.len(),
        doc.styles.styles.len()
    );
    if !doc.numbering.is_empty() {
        let _ = writeln!(
            out,
            "Списков: {} (определений {})",
            doc.numbering.instances.len(),
            doc.numbering.abstracts.len()
        );
    }
    if let Some(title) = &core.title {
        let _ = writeln!(out, "Название: {title:?}");
    }
    if let Some(creator) = &core.creator {
        let _ = writeln!(out, "Автор: {creator:?}");
    }
    let stats = doc.statistics();
    let _ = writeln!(
        out,
        "Статистика: слов {}, знаков {} (с пробелами {}), абзацев {}",
        stats.words, stats.characters, stats.characters_with_spaces, stats.paragraphs
    );
    if let Some(source) = &doc.source {
        let _ = writeln!(out, "Части файла без разбора: {}", source.parts.len());
    }
    for (i, section) in doc.sections.iter().enumerate() {
        dump_section(&mut out, i + 1, section);
    }
    out
}

fn dump_section(out: &mut String, number: usize, section: &Section) {
    let mut line = format!("Раздел {number}");
    if let Some(size) = section.props.page_size {
        let _ = write!(
            line,
            ", страница {:.1}×{:.1} мм",
            size.width.to_mm(),
            size.height.to_mm()
        );
    }
    if let Some(m) = section.props.margins {
        let _ = write!(
            line,
            ", поля {:.1}/{:.1}/{:.1}/{:.1} мм",
            m.top.to_mm(),
            m.right.to_mm(),
            m.bottom.to_mm(),
            m.left.to_mm()
        );
    }
    push_unknown_count(&mut line, section.props.unknown.len());
    let _ = writeln!(out, "{line}");
    for block in &section.blocks {
        dump_block(out, 1, block);
    }
}

fn dump_block(out: &mut String, depth: usize, block: &Block) {
    match block {
        Block::Paragraph(p) => dump_paragraph(out, depth, p),
        Block::Table(t) => dump_table(out, depth, t),
        Block::Unknown(raw) => {
            let _ = writeln!(out, "{}Непонятый блок <{}>", indent(depth), raw.name());
        }
    }
}

fn dump_paragraph(out: &mut String, depth: usize, p: &Paragraph) {
    let props = paragraph_props(&p.props);
    let _ = writeln!(out, "{}Абзац{props}", indent(depth));
    for inline in &p.content {
        match inline {
            Inline::Run(run) => dump_run(out, depth + 1, run),
            Inline::Unknown(raw) => {
                let _ = writeln!(out, "{}Непонятое <{}>", indent(depth + 1), raw.name());
            }
        }
    }
}

fn dump_run(out: &mut String, depth: usize, run: &Run) {
    let mut content = String::new();
    for item in &run.content {
        match item {
            RunContent::Text(text) => content.push_str(text),
            RunContent::Unknown(raw) => {
                let _ = write!(content, "<{}>", raw.name());
            }
            RunContent::Tab => content.push('\t'),
            RunContent::Break(BreakKind::Line) => content.push('\n'),
            RunContent::Break(BreakKind::Page) => content.push_str("<разрыв страницы>"),
            RunContent::Break(BreakKind::Column) => content.push_str("<разрыв колонки>"),
        }
    }
    let props = run_props(&run.props);
    let _ = writeln!(out, "{}Фрагмент{props}: {content:?}", indent(depth));
}

fn dump_table(out: &mut String, depth: usize, table: &Table) {
    let columns = table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
    let _ = writeln!(
        out,
        "{}Таблица {}×{}",
        indent(depth),
        table.rows.len(),
        columns
    );
    for (r, row) in table.rows.iter().enumerate() {
        for (c, cell) in row.cells.iter().enumerate() {
            let _ = writeln!(out, "{}Ячейка {}.{}", indent(depth + 1), r + 1, c + 1);
            for block in &cell.blocks {
                dump_block(out, depth + 2, block);
            }
        }
    }
}

/// Краткое описание оформления абзаца: ` [стиль Heading1, по центру]`.
fn paragraph_props(props: &ParagraphProps) -> String {
    let mut parts = Vec::new();
    if let Some(style) = &props.style {
        parts.push(format!("стиль {style}"));
    }
    if let Some(numbering) = props.numbering {
        let id = numbering.id.map_or("?".to_owned(), |i| i.to_string());
        let level = numbering.level.unwrap_or(0);
        parts.push(format!("список {id}, уровень {level}"));
    }
    if let Some(alignment) = props.alignment {
        parts.push(
            match alignment {
                Alignment::Left => "по левому краю",
                Alignment::Center => "по центру",
                Alignment::Right => "по правому краю",
                Alignment::Justify => "по ширине",
                Alignment::Distribute => "распределённое",
            }
            .to_string(),
        );
    }
    let ind = &props.indentation;
    if let Some(left) = ind.left {
        parts.push(format!("слева {}", left.0));
    }
    if let Some(right) = ind.right {
        parts.push(format!("справа {}", right.0));
    }
    match ind.first_line {
        Some(FirstLineIndent::FirstLine(t)) => parts.push(format!("отступ {}", t.0)),
        Some(FirstLineIndent::Hanging(t)) => parts.push(format!("выступ {}", t.0)),
        None => {}
    }
    let spacing = &props.spacing;
    if let Some(before) = spacing.before {
        parts.push(format!("перед {}", before.0));
    }
    if let Some(after) = spacing.after {
        parts.push(format!("после {}", after.0));
    }
    match spacing.line {
        Some(LineSpacing::Multiple(n)) => {
            parts.push(format!("интервал ×{:.2}", f64::from(n) / 240.0))
        }
        Some(LineSpacing::Exact(t)) => parts.push(format!("интервал точно {}", t.0)),
        Some(LineSpacing::AtLeast(t)) => parts.push(format!("интервал минимум {}", t.0)),
        None => {}
    }
    bracket(parts, props.unknown.len())
}

/// Краткое описание оформления фрагмента: ` [ж, к, 14 пт, Arial]`.
fn run_props(props: &RunProps) -> String {
    let mut parts = Vec::new();
    if let Some(style) = &props.style {
        parts.push(format!("стиль {style}"));
    }
    let toggles = [
        (props.bold, "ж"),
        (props.italic, "к"),
        (props.strike, "зачёркнутый"),
        (props.double_strike, "двойное зачёркивание"),
        (props.caps, "все прописные"),
        (props.small_caps, "малые прописные"),
        (props.hidden, "скрытый"),
    ];
    for (value, name) in toggles {
        match value {
            Some(true) => parts.push(name.to_string()),
            Some(false) => parts.push(format!("не {name}")),
            None => {}
        }
    }
    if let Some(underline) = props.underline {
        parts.push(format!("подчёркивание {underline:?}"));
    }
    if let Some(size) = props.size {
        parts.push(format!("{} пт", f64::from(size.0) / 2.0));
    }
    if let Some(font) = &props.fonts.ascii {
        parts.push(font.clone());
    }
    if let Some(color) = props.color {
        parts.push(format!("цвет {color:?}"));
    }
    if let Some(highlight) = props.highlight {
        parts.push(format!("выделение {highlight:?}"));
    }
    if let Some(spacing) = props.spacing {
        parts.push(format!("интервал между буквами {}", spacing.0));
    }
    if let Some(scale) = props.scale {
        parts.push(format!("масштаб {scale} %"));
    }
    if let Some(position) = props.position {
        parts.push(format!("смещение {position}"));
    }
    if let Some(align) = props.vertical_align {
        parts.push(format!("{align:?}"));
    }
    bracket(parts, props.unknown.len())
}

fn bracket(mut parts: Vec<String>, unknown: usize) -> String {
    if unknown > 0 {
        parts.push(format!("непонятых свойств {unknown}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" [{}]", parts.join(", "))
    }
}

fn push_unknown_count(line: &mut String, unknown: usize) {
    if unknown > 0 {
        let _ = write!(line, ", непонятых свойств {unknown}");
    }
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use letter_model::{HalfPoints, Spacing, Twips};

    #[test]
    fn property_summaries() {
        let run = RunProps {
            bold: Some(true),
            italic: Some(false),
            size: Some(HalfPoints(28)),
            ..RunProps::default()
        };
        assert_eq!(run_props(&run), " [ж, не к, 14 пт]");
        assert_eq!(run_props(&RunProps::default()), "");

        let paragraph = ParagraphProps {
            alignment: Some(Alignment::Center),
            spacing: Spacing {
                line: Some(LineSpacing::ONE_AND_HALF),
                after: Some(Twips(120)),
                ..Spacing::default()
            },
            ..ParagraphProps::default()
        };
        assert_eq!(
            paragraph_props(&paragraph),
            " [по центру, после 120, интервал ×1.50]"
        );
    }

    #[test]
    fn new_document_dump() {
        let out = dump(&Document::new());
        assert_eq!(
            out,
            "Документ: разделов 1, стилей 0\n\
             Статистика: слов 0, знаков 0 (с пробелами 0), абзацев 0\n\
             Раздел 1\n  Абзац\n"
        );
    }
}
