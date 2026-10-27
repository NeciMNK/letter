//! Запись модели документа в ODT.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::{Cursor, Write as _};

use letter_model::{
    Block, BreakKind, Document, FirstLineIndent, Inline, PageMargins, Paragraph, ParagraphProps,
    ResolvedParagraphProps, Run, RunContent, RunFonts, RunProps, Section, StyleKind, Table,
    TableCell, Twips,
};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::WriteError;
use crate::names::{STANDARD, encode, heading_level};
use crate::props::{Attrs, attrs_string, write_paragraph, write_text};
use crate::toggles;
use crate::units::length;
use crate::xml::{NAMESPACES, escape};

const MIME: &str = "application/vnd.oasis.opendocument.text";
/// Настройки совместимости, которые LibreOffice ставит документам из
/// Word (так он открывает .docx). Без них LibreOffice считает файл
/// старым и верстает по-своему: другая высота строк, табуляции и т. п.
const WORD_COMPAT: &[(&str, &str, &str)] = &[
    ("NoNumberingShowFollowBy", "boolean", "true"),
    ("HyphenateURLs", "boolean", "false"),
    ("FootnoteInColumnToPageEnd", "boolean", "true"),
    ("GutterAtTop", "boolean", "false"),
    ("ContinuousEndnotes", "boolean", "false"),
    ("FrameAutowidthWithMorePara", "boolean", "true"),
    ("SubtractFlysAnchoredAtFlys", "boolean", "false"),
    ("SurroundTextWrapSmall", "boolean", "true"),
    ("TreatSingleColumnBreakAsPageBreak", "boolean", "true"),
    ("TabOverSpacing", "boolean", "true"),
    ("AutoFirstLineIndentDisregardLineSpace", "boolean", "true"),
    ("TabOverMargin", "boolean", "true"),
    (
        "ApplyTextAttrToEmptyLineAtEndOfParagraph",
        "boolean",
        "true",
    ),
    ("DisableOffPagePositioning", "boolean", "true"),
    ("BackgroundParaOverDrawings", "boolean", "true"),
    ("TabOverflow", "boolean", "true"),
    ("AddVerticalFrameOffsets", "boolean", "true"),
    ("AddFrameOffsets", "boolean", "true"),
    ("TableRowKeep", "boolean", "true"),
    ("ApplyParagraphMarkFormatToNumbering", "boolean", "true"),
    ("PrinterIndependentLayout", "string", "high-resolution"),
    ("JustifyLinesWithShrinking", "boolean", "false"),
    ("CollapseEmptyCellPara", "boolean", "true"),
    ("UnbreakableNumberings", "boolean", "true"),
    ("MsWordCompTrailingBlanks", "boolean", "true"),
    ("OutlineLevelYieldsNumbering", "boolean", "false"),
    ("UseOldNumbering", "boolean", "false"),
    ("AddParaSpacingToTableCells", "boolean", "true"),
    ("PropLineSpacingShrinksFirstLine", "boolean", "true"),
    ("TabAtLeftIndentForParagraphsInList", "boolean", "true"),
    ("DoNotCaptureDrawObjsOnPage", "boolean", "false"),
    ("HeaderSpacingBelowLastPara", "boolean", "true"),
    ("ClippedPictures", "boolean", "true"),
    ("MathBaselineAlignment", "boolean", "true"),
    ("AlignTabStopPosition", "boolean", "true"),
    ("UseFormerLineSpacing", "boolean", "false"),
    ("DoNotMirrorRtlDrawObjs", "boolean", "true"),
    ("AddParaLineSpacingToTableCells", "boolean", "true"),
    ("IsKernAsianPunctuation", "boolean", "false"),
    ("DoNotJustifyLinesWithManualBreak", "boolean", "false"),
    ("CharacterCompressionType", "short", "0"),
    ("UseVariableWidthNBSP", "boolean", "false"),
    ("AddParaTableSpacingAtStart", "boolean", "true"),
    ("InvertBorderSpacing", "boolean", "true"),
    ("AddExternalLeading", "boolean", "true"),
    ("UseFormerObjectPositioning", "boolean", "false"),
    ("UnxForceZeroExtLeading", "boolean", "false"),
    ("UseFormerTextWrapping", "boolean", "false"),
    ("ConsiderTextWrapOnObjPos", "boolean", "true"),
    ("StylesNoDefault", "boolean", "false"),
    ("IgnoreFirstLineIndentInNumbering", "boolean", "false"),
    ("NoGapAfterNoteNumber", "boolean", "true"),
    ("EmptyDbFieldHidesPara", "boolean", "false"),
    ("DoNotResetParaAttrsForNumFont", "boolean", "false"),
    ("IgnoreTabsAndBlanksForLineCalculation", "boolean", "true"),
    ("ClipAsCharacterAnchoredWriterFlyFrames", "boolean", "false"),
    ("DropCapPunctuation", "boolean", "true"),
    ("UseOldPrinterMetrics", "boolean", "false"),
    ("MsWordCompMinLineHeightByFly", "boolean", "true"),
    ("SmallCapsPercentage66", "boolean", "false"),
];
/// Больше стольких столбцов одна ячейка не занимает (как в Word).
const MAX_SPAN: usize = 63;
/// Внутренние поля ячейки таблицы, как у Word по умолчанию (0,19 см).
const CELL_PADDING: Twips = Twips(108);

pub(crate) fn write(doc: &Document) -> Result<Vec<u8>, WriteError> {
    let mut writer = Writer::new(doc);
    let content = writer.content();
    let styles = writer.styles();
    let parts: [(&str, String); 5] = [
        ("content.xml", content),
        ("styles.xml", styles),
        ("meta.xml", meta(doc)),
        ("settings.xml", settings(doc)),
        ("META-INF/manifest.xml", manifest(&writer.media)),
    ];

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    // `mimetype` — первым и без сжатия: по нему программы узнают формат.
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("mimetype", stored)?;
    zip.write_all(MIME.as_bytes())?;
    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, xml) in parts {
        zip.start_file(name, deflated)?;
        zip.write_all(xml.as_bytes())?;
    }
    // Картинки уже сжаты — без сжатия, как у LibreOffice.
    for (path, bytes, _) in &writer.media {
        zip.start_file(path.as_str(), stored)?;
        zip.write_all(bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}

/// Автоматические стили: одинаковые записываются один раз.
#[derive(Default)]
struct AutoStyles {
    xml: String,
    names: HashMap<String, String>,
    counters: HashMap<&'static str, usize>,
}

impl AutoStyles {
    /// Имя стиля с таким содержимым (новое, если такого ещё нет).
    fn get(&mut self, prefix: &'static str, family: &str, attrs: &str, inner: &str) -> String {
        let key = format!("{family}|{attrs}|{inner}");
        if let Some(name) = self.names.get(&key) {
            return name.clone();
        }
        let n = self.counters.entry(prefix).or_insert(0);
        *n += 1;
        let name = format!("{prefix}{n}");
        let _ = write!(
            self.xml,
            "<style:style style:name=\"{name}\" style:family=\"{family}\"{attrs}>{inner}</style:style>"
        );
        self.names.insert(key, name.clone());
        name
    }
}

struct Writer<'a> {
    doc: &'a Document,
    /// Имена стилей в ODF: (вид, идентификатор модели) → `style:name`.
    names: HashMap<(StyleKind, String), String>,
    fonts: BTreeSet<String>,
    auto: AutoStyles,
    /// Разрыв, который надо поставить перед следующим абзацем.
    pending_break: Option<BreakKind>,
    /// Шаблон страницы, с которого начинается следующий блок.
    pending_master: Option<String>,
    tables: usize,
    /// Картинки для папки `Pictures/`: путь в архиве, байты, MIME-тип.
    media: Vec<(String, Vec<u8>, &'static str)>,
    /// Ширина текста текущего раздела (для таблиц).
    text_width: Twips,
}

impl<'a> Writer<'a> {
    fn new(doc: &'a Document) -> Writer<'a> {
        let mut names = HashMap::new();
        let mut taken: std::collections::HashSet<(StyleKind, String)> = Default::default();
        let default_paragraph = doc
            .styles
            .default_for(StyleKind::Paragraph)
            .map(|s| s.id.clone());
        for style in &doc.styles.styles {
            let kind = style.kind;
            if !matches!(kind, StyleKind::Paragraph | StyleKind::Character)
                || names.contains_key(&(kind, style.id.clone()))
            {
                continue;
            }
            let wanted = if default_paragraph.as_ref() == Some(&style.id) {
                STANDARD.to_string()
            } else if let Some(level) = style.name.as_deref().and_then(heading_level) {
                format!("Heading_20_{level}")
            } else {
                encode(&style.id)
            };
            let mut name = wanted.clone();
            let mut n = 1;
            while taken.contains(&(kind, name.clone())) {
                n += 1;
                name = format!("{wanted}{n}");
            }
            taken.insert((kind, name.clone()));
            names.insert((kind, style.id.clone()), name);
        }
        Writer {
            doc,
            names,
            fonts: BTreeSet::new(),
            auto: AutoStyles::default(),
            pending_break: None,
            pending_master: None,
            tables: 0,
            media: Vec::new(),
            text_width: Twips(9000),
        }
    }

    fn name(&self, kind: StyleKind, id: &str) -> Option<&str> {
        self.names.get(&(kind, id.to_string())).map(String::as_str)
    }

    fn content(&mut self) -> String {
        let mut body = String::new();
        for (i, section) in self.doc.sections.iter().enumerate() {
            let (size, margins) = page(section);
            self.text_width = Twips((size.width.0 - margins.left.0 - margins.right.0).max(1440));
            self.pending_master = (i > 0).then(|| master_name(i));
            let mut written = false;
            for block in &section.blocks {
                written |= self.block(block, &mut body, true);
            }
            if !written {
                self.paragraph(&Paragraph::default(), &mut body, true);
            }
        }
        if self.pending_break.take().is_some() {
            self.paragraph(&Paragraph::default(), &mut body, true);
        }

        let mut out = xml_head("office:document-content");
        out.push_str(&self.font_decls());
        let _ = write!(
            out,
            "<office:automatic-styles>{}</office:automatic-styles>",
            self.auto.xml
        );
        let _ = write!(
            out,
            "<office:body><office:text>{body}</office:text></office:body></office:document-content>"
        );
        out
    }

    /// Записывает блок. Возвращает `false`, если блок не записан
    /// (непонятый элемент).
    fn block(&mut self, block: &Block, out: &mut String, top: bool) -> bool {
        match block {
            Block::Paragraph(p) => {
                self.paragraph(p, out, top);
                true
            }
            Block::Table(t) => {
                self.table(t, out, top);
                true
            }
            Block::Unknown(_) => false,
        }
    }

    fn paragraph(&mut self, p: &Paragraph, out: &mut String, top: bool) {
        // Разрыв страницы внутри абзаца ODF не умеет: абзац делится,
        // а следующая часть начинается с новой страницы.
        let mut pieces: Vec<(Vec<Run>, Option<BreakKind>)> = vec![(Vec::new(), None)];
        for item in &p.content {
            let Inline::Run(run) = item else { continue };
            let mut current = Run {
                props: run.props.clone(),
                content: Vec::new(),
            };
            for content in &run.content {
                match content {
                    RunContent::Break(kind @ (BreakKind::Page | BreakKind::Column)) => {
                        if let Some(last) = pieces.last_mut()
                            && !current.content.is_empty()
                        {
                            last.0.push(std::mem::replace(
                                &mut current,
                                Run {
                                    props: run.props.clone(),
                                    content: Vec::new(),
                                },
                            ));
                        }
                        pieces.push((Vec::new(), Some(*kind)));
                    }
                    other => current.content.push(other.clone()),
                }
            }
            if let Some(last) = pieces.last_mut()
                && !current.content.is_empty()
            {
                last.0.push(current);
            }
        }
        // Разрыв в конце предыдущего абзаца относится к первой части.
        let mut incoming = self.pending_break.take();
        let trailing = pieces.len() > 1 && pieces.last().is_some_and(|(runs, _)| runs.is_empty());
        if trailing {
            // Разрыв в самом конце абзаца: с новой страницы начнётся
            // следующий абзац.
            self.pending_break = pieces.pop().and_then(|(_, brk)| brk);
        }
        let last = pieces.len().saturating_sub(1);
        for (i, (runs, brk)) in pieces.into_iter().enumerate() {
            let mut props = p.props.clone();
            // Части одного абзаца Word: интервалов между ними нет.
            if i > 0 {
                props.spacing.before = Some(Twips::ZERO);
            }
            if i < last {
                props.spacing.after = Some(Twips::ZERO);
            }
            let brk = match brk {
                Some(brk) => Some(brk),
                None => {
                    let pending = incoming.take();
                    // После разрыва страницы Word не добавляет интервал
                    // перед абзацем вверху новой страницы.
                    if pending.is_some() {
                        props.spacing.before = Some(Twips::ZERO);
                    }
                    pending
                }
            };
            self.single_paragraph(&props, &runs, brk, out, top);
        }
    }

    fn single_paragraph(
        &mut self,
        props: &ParagraphProps,
        runs: &[Run],
        brk: Option<BreakKind>,
        out: &mut String,
        top: bool,
    ) {
        let doc = self.doc;
        let style_id = props
            .style
            .clone()
            .or_else(|| doc.resolve_paragraph(props).style);
        let parent = style_id
            .as_deref()
            .and_then(|id| self.name(StyleKind::Paragraph, id))
            .map(str::to_string);
        let mut direct = props.clone();
        direct.style = None;
        complete(&mut direct, &doc.resolve_paragraph(props));
        let mut column = false;
        match brk {
            Some(BreakKind::Page) => direct.page_break_before = Some(true),
            Some(BreakKind::Column) => column = true,
            _ => {}
        }
        let (mut attrs, inner) = write_paragraph(&direct);
        if column {
            attrs.retain(|(n, _)| *n != "fo:break-before");
            attrs.push(("fo:break-before", "column".into()));
        }
        let master = if top {
            self.pending_master.take()
        } else {
            None
        };

        let style = if attrs.is_empty() && inner.is_empty() && master.is_none() {
            parent.clone()
        } else {
            let mut head: Attrs = Vec::new();
            if let Some(parent) = &parent {
                head.push(("style:parent-style-name", parent.clone()));
            }
            if let Some(master) = master {
                head.push(("style:master-page-name", master));
            }
            let body = element("style:paragraph-properties", &attrs, &inner);
            Some(self.auto.get("P", "paragraph", &attrs_string(&head), &body))
        };

        let level = style_id
            .as_deref()
            .and_then(|id| doc.styles.get(id))
            .and_then(|s| s.name.as_deref())
            .and_then(heading_level);
        let (tag, extra) = match level {
            Some(level) => ("text:h", format!(" text:outline-level=\"{level}\"")),
            None => ("text:p", String::new()),
        };
        let style_attr = style
            .map(|s| format!(" text:style-name=\"{}\"", escape(&s)))
            .unwrap_or_default();
        let mut text = Text::default();
        for run in runs {
            self.run(props, run, &mut text);
        }
        if text.out.is_empty() {
            let _ = write!(out, "<{tag}{style_attr}{extra}/>");
        } else {
            let _ = write!(out, "<{tag}{style_attr}{extra}>{}</{tag}>", text.out);
        }
    }

    fn run(&mut self, paragraph: &ParagraphProps, run: &Run, text: &mut Text) {
        let mut props = run.props.clone();
        toggles::fix(self.doc, paragraph, &mut props);
        let char_style = props
            .style
            .take()
            .and_then(|id| self.name(StyleKind::Character, &id).map(str::to_string));
        let attrs = write_text(&props, self.doc, &mut self.fonts);
        let style = if attrs.is_empty() {
            char_style
        } else {
            let head = char_style
                .map(|c| format!(" style:parent-style-name=\"{}\"", escape(&c)))
                .unwrap_or_default();
            let body = element("style:text-properties", &attrs, "");
            Some(self.auto.get("T", "text", &head, &body))
        };
        if let Some(style) = &style {
            let _ = write!(
                text.out,
                "<text:span text:style-name=\"{}\">",
                escape(style)
            );
        }
        for item in &run.content {
            match item {
                RunContent::Text(s) => text.push(s),
                RunContent::Symbol(s) => text.push(&s.unicode().to_string()),
                // Формулы ODT (MathML-объекты) пока не пишем: линейный вид.
                RunContent::Math(m) => text.push(&m.linear_text().replace('\n', " ")),
                RunContent::Tab => text.element("<text:tab/>"),
                RunContent::Break(_) => text.element("<text:line-break/>"),
                RunContent::Picture(picture) => {
                    if let Some(frame) = self.picture(picture) {
                        text.element(&frame);
                    }
                }
                RunContent::FieldChar(_)
                | RunContent::FieldCode(_)
                | RunContent::NoteReference(_)
                | RunContent::NoteMark(_)
                | RunContent::CommentReference(_)
                | RunContent::Unknown(_) => {}
            }
        }
        if style.is_some() {
            text.out.push_str("</text:span>");
        }
    }

    /// Рисунок в строке: `draw:frame` с картинкой из `Pictures/`. Фигуры,
    /// диаграммы и картинки незнакомого вида не записываются.
    fn picture(&mut self, picture: &letter_model::Picture) -> Option<String> {
        let image = picture.image.as_ref().filter(|i| i.chart.is_none())?;
        let (extension, mime) = image_kind(&image.bytes)?;
        let path = format!("Pictures/image{}.{extension}", self.media.len() + 1);
        self.media.push((path.clone(), image.bytes.clone(), mime));
        let n = self.media.len();
        Some(format!(
            "<draw:frame draw:name=\"Image{n}\" text:anchor-type=\"as-char\" \
             svg:width=\"{}pt\" svg:height=\"{}pt\" draw:z-index=\"0\">\
             <draw:image xlink:href=\"{path}\" xlink:type=\"simple\" \
             xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>",
            picture.width_points(),
            picture.height_points()
        ))
    }

    fn table(&mut self, table: &Table, out: &mut String, top: bool) {
        self.tables += 1;
        let name = format!("Table{}", self.tables);
        let columns = table
            .rows
            .iter()
            .map(|r| r.cells.iter().map(grid_span).sum::<usize>())
            .max()
            .unwrap_or(0)
            .max(1);
        let width = self.text_width;

        let mut head: Attrs = Vec::new();
        if top && let Some(master) = self.pending_master.take() {
            head.push(("style:master-page-name", master));
        }
        let mut props: Attrs = vec![
            ("style:width", length(width)),
            ("table:align", "left".into()),
        ];
        if top && self.pending_break.take().is_some() {
            props.push(("fo:break-before", "page".into()));
        }
        let table_style = self.auto.get(
            "Table",
            "table",
            &attrs_string(&head),
            &element("style:table-properties", &props, ""),
        );
        let column_width = Twips(width.0 / columns as i32);
        let column_style = self.auto.get(
            "Column",
            "table-column",
            "",
            &element(
                "style:table-column-properties",
                &vec![("style:column-width", length(column_width))],
                "",
            ),
        );
        let padding = length(CELL_PADDING);
        let cell_style = self.auto.get(
            "Cell",
            "table-cell",
            "",
            &element(
                "style:table-cell-properties",
                &vec![
                    ("fo:padding-left", padding.clone()),
                    ("fo:padding-right", padding),
                    ("fo:padding-top", "0pt".into()),
                    ("fo:padding-bottom", "0pt".into()),
                    ("fo:border", "0.5pt solid #000000".into()),
                ],
                "",
            ),
        );

        let _ = write!(
            out,
            "<table:table table:name=\"{name}\" table:style-name=\"{table_style}\">\
             <table:table-column table:style-name=\"{column_style}\" \
             table:number-columns-repeated=\"{columns}\"/>"
        );
        for row in &table.rows {
            out.push_str("<table:table-row>");
            for cell in &row.cells {
                let span = grid_span(cell);
                let spanned = if span > 1 {
                    format!(" table:number-columns-spanned=\"{span}\"")
                } else {
                    String::new()
                };
                let _ = write!(
                    out,
                    "<table:table-cell table:style-name=\"{cell_style}\"{spanned} office:value-type=\"string\">"
                );
                let mut written = false;
                for block in &cell.blocks {
                    written |= self.block(block, out, false);
                }
                if !written {
                    out.push_str("<text:p/>");
                }
                out.push_str("</table:table-cell>");
                for _ in 1..span {
                    out.push_str("<table:covered-table-cell/>");
                }
            }
            out.push_str("</table:table-row>");
        }
        out.push_str("</table:table>");
    }

    /// `styles.xml`: стили, параметры страниц, шрифты.
    fn styles(&mut self) -> String {
        let doc = self.doc;
        let mut styles = String::new();

        // Стиль по умолчанию — полностью, со значениями по умолчанию модели:
        // у LibreOffice свои умолчания (другой шрифт и размер).
        let mut base = letter_model::Document::new();
        base.styles.defaults = doc.styles.defaults.clone();
        base.styles.styles.clear();
        base.theme = doc.theme.clone();
        let para = base.resolve_paragraph(&ParagraphProps::default());
        let run = base.resolve_run(&ParagraphProps::default(), &RunProps::default());
        let para = ParagraphProps {
            alignment: Some(para.alignment),
            indentation: letter_model::Indentation {
                left: Some(para.indent_left),
                right: Some(para.indent_right),
                first_line: Some(
                    para.first_line
                        .unwrap_or(FirstLineIndent::FirstLine(Twips::ZERO)),
                ),
            },
            spacing: letter_model::Spacing {
                before: Some(para.space_before),
                after: Some(para.space_after),
                line: Some(para.line_spacing),
            },
            contextual_spacing: Some(para.contextual_spacing),
            keep_next: Some(para.keep_next),
            keep_lines: Some(para.keep_lines),
            page_break_before: Some(para.page_break_before),
            widow_control: Some(para.widow_control),
            tabs: para.tabs,
            ..ParagraphProps::default()
        };
        let run = RunProps {
            fonts: RunFonts {
                ascii: Some(run.fonts.ascii.clone()),
                h_ansi: Some(run.fonts.h_ansi),
                east_asia: Some(run.fonts.east_asia),
                complex_script: Some(run.fonts.complex_script),
                ..RunFonts::default()
            },
            size: Some(run.size),
            bold: Some(run.bold),
            italic: Some(run.italic),
            underline: Some(run.underline),
            strike: Some(run.strike),
            double_strike: Some(run.double_strike),
            color: Some(run.color),
            highlight: Some(run.highlight),
            vertical_align: Some(run.vertical_align),
            ..RunProps::default()
        };
        let (mut p_attrs, p_inner) = write_paragraph(&para);
        p_attrs.push((
            "style:tab-stop-distance",
            length(doc.settings.default_tab_stop),
        ));
        let t_attrs = write_text(&run, doc, &mut self.fonts);
        let _ = write!(
            styles,
            "<style:default-style style:family=\"paragraph\">{}{}</style:default-style>",
            element("style:paragraph-properties", &p_attrs, &p_inner),
            element("style:text-properties", &t_attrs, ""),
        );

        let mut written = std::collections::HashSet::new();
        for style in &doc.styles.styles {
            let (family, kind) = match style.kind {
                StyleKind::Paragraph => ("paragraph", StyleKind::Paragraph),
                StyleKind::Character => ("text", StyleKind::Character),
                _ => continue,
            };
            let Some(name) = self.name(kind, &style.id).map(str::to_string) else {
                continue;
            };
            // Стиль с тем же идентификатором встречается второй раз.
            if !written.insert((kind, style.id.clone())) {
                continue;
            }
            let mut head: Attrs = vec![("style:name", name.clone())];
            if let Some(display) = &style.name
                && *display != name
            {
                head.push(("style:display-name", display.clone()));
            }
            head.push(("style:family", family.into()));
            let parent = style
                .based_on
                .as_deref()
                .and_then(|id| self.name(kind, id))
                .filter(|p| *p != name);
            if let Some(parent) = parent {
                head.push(("style:parent-style-name", parent.to_string()));
            }
            if let Some(next) = style
                .next
                .as_deref()
                .and_then(|id| self.name(StyleKind::Paragraph, id))
                .filter(|_| kind == StyleKind::Paragraph)
            {
                head.push(("style:next-style-name", next.to_string()));
            }
            if let Some(level) = style.name.as_deref().and_then(heading_level)
                && kind == StyleKind::Paragraph
            {
                head.push(("style:default-outline-level", level.to_string()));
                // Без нумерации глав LibreOffice (у неё свои отступы).
                head.push(("style:list-style-name", String::new()));
            }
            let mut body = String::new();
            if kind == StyleKind::Paragraph {
                let mut props = style.paragraph.clone();
                props.style = None;
                let resolved = doc.resolve_paragraph(&ParagraphProps {
                    style: Some(style.id.clone()),
                    ..ParagraphProps::default()
                });
                complete(&mut props, &resolved);
                let (attrs, inner) = write_paragraph(&props);
                if !attrs.is_empty() || !inner.is_empty() {
                    body.push_str(&element("style:paragraph-properties", &attrs, &inner));
                }
            }
            let mut run = style.run.clone();
            run.style = None;
            let attrs = write_text(&run, doc, &mut self.fonts);
            if !attrs.is_empty() {
                body.push_str(&element("style:text-properties", &attrs, ""));
            }
            styles.push_str(&element("style:style", &head, &body));
        }

        let mut layouts = String::new();
        let mut masters = String::new();
        for (i, section) in doc.sections.iter().enumerate() {
            let layout = format!("pm{}", i + 1);
            let (size, margins) = page(section);
            let mut attrs: Attrs = vec![
                ("fo:page-width", length(size.width)),
                ("fo:page-height", length(size.height)),
                (
                    "style:print-orientation",
                    match size.orientation {
                        letter_model::Orientation::Portrait => "portrait",
                        letter_model::Orientation::Landscape => "landscape",
                    }
                    .into(),
                ),
                ("fo:margin-top", length(margins.top)),
                ("fo:margin-bottom", length(margins.bottom)),
                ("fo:margin-left", length(margins.left)),
                ("fo:margin-right", length(margins.right)),
            ];
            if margins.gutter != Twips::ZERO {
                attrs.push(("loext:margin-gutter", length(margins.gutter)));
            }
            let _ = write!(
                layouts,
                "<style:page-layout style:name=\"{layout}\">{}</style:page-layout>",
                element("style:page-layout-properties", &attrs, "")
            );
            let _ = write!(
                masters,
                "<style:master-page style:name=\"{}\" style:page-layout-name=\"{layout}\"/>",
                master_name(i)
            );
        }
        if doc.sections.is_empty() {
            layouts.push_str("<style:page-layout style:name=\"pm1\"/>");
            masters.push_str(
                "<style:master-page style:name=\"Standard\" style:page-layout-name=\"pm1\"/>",
            );
        }

        let mut out = xml_head("office:document-styles");
        out.push_str(&self.font_decls());
        let _ = write!(
            out,
            "<office:styles>{styles}</office:styles>\
             <office:automatic-styles>{layouts}</office:automatic-styles>\
             <office:master-styles>{masters}</office:master-styles>\
             </office:document-styles>"
        );
        out
    }

    fn font_decls(&self) -> String {
        let mut out = String::from("<office:font-face-decls>");
        for font in &self.fonts {
            let font = escape(font);
            let _ = write!(
                out,
                "<style:font-face style:name=\"{font}\" svg:font-family=\"&apos;{font}&apos;\"/>"
            );
        }
        out.push_str("</office:font-face-decls>");
        out
    }
}

/// Текст абзаца в XML: пробелы, которые XML схлопнул бы, пишутся
/// через `text:s`.
#[derive(Default)]
struct Text {
    out: String,
    /// Предыдущий записанный знак — пробел, табуляция или начало абзаца.
    after_space: bool,
    started: bool,
}

impl Text {
    fn element(&mut self, xml: &str) {
        self.out.push_str(xml);
        self.started = true;
        self.after_space = true;
    }

    fn push(&mut self, s: &str) {
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            match c {
                ' ' => {
                    let mut n = 0;
                    while i + n < chars.len() && chars[i + n] == ' ' {
                        n += 1;
                    }
                    let followed = i + n < chars.len();
                    let mut spaces = n;
                    if self.started && !self.after_space && followed {
                        self.out.push(' ');
                        spaces -= 1;
                    }
                    match spaces {
                        0 => {}
                        1 => self.out.push_str("<text:s/>"),
                        n => {
                            let _ = write!(self.out, "<text:s text:c=\"{n}\"/>");
                        }
                    }
                    self.started = true;
                    self.after_space = true;
                    i += n;
                    continue;
                }
                '\t' => self.element("<text:tab/>"),
                '\n' | '\u{b}' => self.element("<text:line-break/>"),
                '\r' => {}
                c => {
                    self.out.push_str(&escape(c.encode_utf8(&mut [0; 4])));
                    self.started = true;
                    self.after_space = false;
                }
            }
            i += 1;
        }
    }
}

/// Сколько столбцов сетки занимает ячейка (`w:gridSpan`).
fn grid_span(cell: &TableCell) -> usize {
    cell.span().clamp(1, MAX_SPAN)
}

/// LibreOffice хранит интервалы до и после абзаца одним свойством, а
/// отступы слева, справа и первой строки — другим. Если в файле задана
/// только часть такого свойства, остальное он берёт не от родительского
/// стиля, а нулевым. Поэтому недостающее дописывается итоговым значением.
/// У пунктов списка отступы приходят из уровня списка, которого в ODT
/// пока нет, — их тоже пишем итоговыми.
fn complete(props: &mut ParagraphProps, resolved: &ResolvedParagraphProps) {
    let spacing = &mut props.spacing;
    if spacing.before.is_some() || spacing.after.is_some() {
        spacing.before.get_or_insert(resolved.space_before);
        spacing.after.get_or_insert(resolved.space_after);
    }
    let indent = &mut props.indentation;
    if indent.left.is_some()
        || indent.right.is_some()
        || indent.first_line.is_some()
        || resolved.numbering.is_some()
    {
        indent.left.get_or_insert(resolved.indent_left);
        indent.right.get_or_insert(resolved.indent_right);
        indent.first_line.get_or_insert(
            resolved
                .first_line
                .unwrap_or(FirstLineIndent::FirstLine(Twips::ZERO)),
        );
    }
}

fn element(name: &str, attrs: &Attrs, inner: &str) -> String {
    if inner.is_empty() {
        format!("<{name}{}/>", attrs_string(attrs))
    } else {
        format!("<{name}{}>{inner}</{name}>", attrs_string(attrs))
    }
}

fn master_name(section: usize) -> String {
    if section == 0 {
        STANDARD.into()
    } else {
        format!("Section{}", section + 1)
    }
}

fn page(section: &Section) -> (letter_model::PageSize, PageMargins) {
    (
        section.props.page_size.unwrap_or_default(),
        section.props.margins.unwrap_or_default(),
    )
}

/// Начало части: XML-объявление и корневой элемент со всеми
/// пространствами имён.
fn xml_head(root: &str) -> String {
    let mut out = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<{root}");
    for (prefix, uri) in NAMESPACES {
        let _ = write!(out, " xmlns:{prefix}=\"{uri}\"");
    }
    out.push_str(" office:version=\"1.3\">");
    out
}

fn meta(doc: &Document) -> String {
    let core = &doc.properties.core;
    let app = &doc.properties.app;
    let mut out = xml_head("office:document-meta");
    out.push_str("<office:meta><meta:generator>letter</meta:generator>");
    let mut field = |name: &str, value: &Option<String>| {
        if let Some(value) = value {
            let _ = write!(out, "<{name}>{}</{name}>", escape(value));
        }
    };
    field("dc:title", &core.title);
    field("dc:subject", &core.subject);
    field("dc:description", &core.description);
    field("meta:keyword", &core.keywords);
    field("meta:initial-creator", &core.creator);
    field("dc:creator", &core.last_modified_by);
    field("meta:creation-date", &core.created);
    field("dc:date", &core.modified);
    field("meta:print-date", &core.last_printed);
    field("meta:editing-cycles", &core.revision);
    field("dc:language", &core.language);
    if let Some(minutes) = app.total_time {
        let _ = write!(
            out,
            "<meta:editing-duration>PT{}H{}M</meta:editing-duration>",
            minutes / 60,
            minutes % 60
        );
    }
    if let Some(template) = &app.template {
        let _ = write!(
            out,
            "<meta:template xlink:type=\"simple\" xlink:href=\"\" xlink:title=\"{}\"/>",
            escape(template)
        );
    }
    let stats = doc.statistics();
    out.push_str("<meta:document-statistic");
    if let Some(pages) = app.pages {
        let _ = write!(out, " meta:page-count=\"{pages}\"");
    }
    let _ = write!(
        out,
        " meta:paragraph-count=\"{}\" meta:word-count=\"{}\" meta:character-count=\"{}\" \
         meta:non-whitespace-character-count=\"{}\"/>",
        stats.paragraphs, stats.words, stats.characters_with_spaces, stats.characters
    );
    out.push_str("</office:meta></office:document-meta>");
    out
}

fn settings(doc: &Document) -> String {
    let mut out = xml_head("office:document-settings");
    let item = |name: &str, kind: &str, value: &str| {
        format!(
            "<config:config-item config:name=\"{name}\" config:type=\"{kind}\">{value}</config:config-item>"
        )
    };
    out.push_str(
        "<office:settings><config:config-item-set config:name=\"ooo:configuration-settings\">",
    );
    // Складывать ли интервал после абзаца с интервалом перед следующим.
    let add = doc.settings.add_paragraph_spacing.to_string();
    out.push_str(&item("AddParaTableSpacing", "boolean", &add));
    // Позиции табуляции — от поля страницы, как в Word.
    out.push_str(&item("TabsRelativeToIndent", "boolean", "false"));
    for (name, kind, value) in WORD_COMPAT {
        out.push_str(&item(name, kind, value));
    }
    out.push_str("</config:config-item-set></office:settings></office:document-settings>");
    out
}

/// Вид картинки по первым байтам: расширение и MIME-тип.
fn image_kind(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG") {
        Some(("png", "image/png"))
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Some(("jpg", "image/jpeg"))
    } else if bytes.starts_with(b"GIF8") {
        Some(("gif", "image/gif"))
    } else if bytes.starts_with(b"BM") {
        Some(("bmp", "image/bmp"))
    } else {
        None
    }
}

fn manifest(media: &[(String, Vec<u8>, &'static str)]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest \
         xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" \
         manifest:version=\"1.3\">",
    );
    let _ = write!(
        out,
        "<manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{MIME}\"/>"
    );
    for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
        let _ = write!(
            out,
            "<manifest:file-entry manifest:full-path=\"{part}\" manifest:media-type=\"text/xml\"/>"
        );
    }
    for (path, _, mime) in media {
        let _ = write!(
            out,
            "<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"{mime}\"/>",
            escape(path)
        );
    }
    out.push_str("</manifest:manifest>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(parts: &[&str]) -> String {
        let mut t = Text::default();
        for p in parts {
            t.push(p);
        }
        t.out
    }

    #[test]
    fn spaces_survive_xml() {
        assert_eq!(text(&["a b"]), "a b");
        assert_eq!(text(&[" a"]), "<text:s/>a");
        assert_eq!(text(&["a  b"]), "a <text:s/>b");
        assert_eq!(text(&["a   "]), "a<text:s text:c=\"3\"/>");
        assert_eq!(text(&["a ", "b"]), "a<text:s/>b");
        assert_eq!(text(&["a\tb<"]), "a<text:tab/>b&lt;");
    }
}
