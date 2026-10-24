//! Оглавление: поле TOC, результат которого — абзацы со ссылками на
//! заголовки документа и номерами их страниц.
//!
//! Поле начинается в первом абзаце оглавления (знак начала, код
//! ` TOC \o "1-3" \h \z \u `, разделитель), дальше идут строки оглавления,
//! а знак конца стоит в отдельном абзаце после них — как пишет Word.
//! Строка оглавления — абзац стиля «Оглавление N» с правой табуляцией
//! с точками: текст заголовка, табуляция и поле PAGEREF с номером
//! страницы. Заголовок получает закладку `_Toc…`, на неё ссылаются и
//! PAGEREF, и ссылка (ключ `\h`), по которой можно перейти к заголовку.

use letter_model::{
    Block, Document, FieldChar, FieldCharKind, Hyperlink, Inline, LineSpacing, OBJECT_CHAR,
    Paragraph, ParagraphProps, RawXml, Run, RunContent, RunProps, Spacing, Style, StyleKind,
    TabLeader, TabStop, TabStopKind, Twips,
};

use crate::edit::{
    paragraph_count, paragraph_mut, paragraph_ref, plain_text, split_paragraph, tidy, top_level,
};
use crate::fields::FieldContext;
use crate::links::add_bookmark;
use crate::notes::ensure_style;
use crate::position::Position;

/// Код нового оглавления: заголовки 1–3, ссылки, без номеров страниц
/// в веб-виде, уровни структуры абзацев.
pub const TOC_CODE: &str = " TOC \\o \"1-3\" \\h \\z \\u ";

/// Уровень заголовка абзаца с оформлением `props` (1–9): из «Уровня»
/// абзаца или его стиля (`w:outlineLvl`), иначе из стиля «Заголовок N».
/// `None` — основной текст.
pub fn outline_level(doc: &Document, props: &ParagraphProps) -> Option<u8> {
    if let Some(level) = raw_level(&props.unknown) {
        return level;
    }
    let mut id = props.style.clone().or_else(|| {
        doc.styles
            .default_for(StyleKind::Paragraph)
            .map(|s| s.id.clone())
    });
    // Цепочка «основан на» (с защитой от петель).
    for _ in 0..16 {
        let style = doc.styles.get(id.as_deref()?)?;
        if let Some(level) = raw_level(&style.paragraph.unknown) {
            return level;
        }
        if let Some(level) = heading_number(style) {
            return Some(level);
        }
        id = style.based_on.clone();
    }
    None
}

/// Уровень из `w:outlineLvl` среди непонятых свойств: `Some(None)` —
/// основной текст (значение 9 и больше).
fn raw_level(unknown: &[RawXml]) -> Option<Option<u8>> {
    let raw = unknown.iter().find(|r| r.local_name() == "outlineLvl")?;
    let value: u8 = raw.attribute("val")?.parse().ok()?;
    Some((value < 9).then_some(value + 1))
}

/// Номер встроенного стиля «heading N».
fn heading_number(style: &Style) -> Option<u8> {
    let name = style.name.as_deref()?.to_ascii_lowercase();
    let number: u8 = name.strip_prefix("heading ")?.trim().parse().ok()?;
    (1..=9).contains(&number).then_some(number)
}

/// Поле TOC в документе: абзацы, где его начало и конец, и код.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TocField {
    first: usize,
    last: usize,
    code: String,
}

/// Оглавления документа по порядку.
fn toc_fields(doc: &Document) -> Vec<TocField> {
    let mut found = Vec::new();
    // Открытые поля: абзац начала, код, прошёл ли разделитель.
    let mut open: Vec<(usize, String, bool)> = Vec::new();
    for index in 0..paragraph_count(doc) {
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        for item in paragraph.runs().flat_map(|r| &r.content) {
            match item {
                RunContent::FieldChar(c) => match c.kind {
                    FieldCharKind::Begin => open.push((index, String::new(), false)),
                    FieldCharKind::Separate => {
                        if let Some(field) = open.last_mut() {
                            field.2 = true;
                        }
                    }
                    FieldCharKind::End => {
                        if let Some((first, code, _)) = open.pop()
                            && open.is_empty()
                            && field_name(&code) == "TOC"
                        {
                            found.push(TocField {
                                first,
                                last: index,
                                code,
                            });
                        }
                    }
                },
                RunContent::FieldCode(code) => {
                    if let Some((_, text, false)) = open.last_mut() {
                        text.push_str(code);
                    }
                }
                _ => {}
            }
        }
    }
    found
}

/// «Удалить оглавление»: убирает оглавление, в котором абзац `at`
/// (если курсор не в оглавлении — первое в документе), вместе с его
/// заголовком «Оглавление» перед ним. `false` — удалять нечего (нет
/// оглавления или оно не из целых абзацев верхнего уровня).
pub fn delete_toc(doc: &mut Document, at: usize) -> bool {
    let tocs = toc_fields(doc);
    let Some(toc) = tocs
        .iter()
        .find(|t| (t.first..=t.last).contains(&at))
        .or_else(|| tocs.first())
    else {
        return false;
    };
    let (Some((section, start)), Some((end_section, end))) =
        (top_level(doc, toc.first), top_level(doc, toc.last))
    else {
        return false;
    };
    if section != end_section || end - start != toc.last - toc.first {
        return false;
    }
    // Заголовок оглавления — абзац стиля «Заголовок оглавления» сразу
    // перед полем.
    let is_title = |doc: &Document| {
        let index = toc.first.checked_sub(1)?;
        let (s, block) = top_level(doc, index)?;
        let style = doc
            .styles
            .get(paragraph_ref(doc, index)?.props.style.as_deref()?)?;
        let title = style.id.eq_ignore_ascii_case("TOCHeading")
            || style
                .name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case("TOC Heading"));
        (title && s == section && block + 1 == start).then_some(block)
    };
    let from = is_title(doc).unwrap_or(start);
    let blocks = &mut doc.sections[section].blocks;
    blocks.drain(from..=end);
    if blocks.is_empty() {
        blocks.push(Paragraph::default().into());
    }
    true
}

/// Имя поля прописными по коду.
fn field_name(code: &str) -> String {
    code.split_whitespace()
        .next()
        .unwrap_or_default()
        .to_uppercase()
}

/// Уровни заголовков из ключа `\o "1-3"` (по умолчанию 1–9).
fn levels(code: &str) -> (u8, u8) {
    let words: Vec<&str> = code.split_whitespace().collect();
    let range = words
        .iter()
        .position(|w| w.eq_ignore_ascii_case("\\o"))
        .and_then(|k| words.get(k + 1))
        .map(|w| w.trim_matches('"'));
    let parsed = range.and_then(|r| {
        let (from, to) = r.split_once('-')?;
        Some((from.trim().parse().ok()?, to.trim().parse().ok()?))
    });
    match parsed {
        Some((from, to)) if from >= 1 && from <= to => (from, to.min(9)),
        _ => (1, 9),
    }
}

/// Есть ли у кода ключ `switch`.
fn has_switch(code: &str, switch: &str) -> bool {
    code.split_whitespace()
        .any(|w| w.eq_ignore_ascii_case(switch))
}

/// Заголовок для оглавления: номер абзаца, уровень и текст.
pub(crate) struct Heading {
    pub(crate) paragraph: usize,
    pub(crate) level: u8,
    pub(crate) text: String,
}

/// Заголовки документа уровней `levels`, кроме абзацев `skip`.
pub(crate) fn headings(
    doc: &Document,
    levels: (u8, u8),
    skip: &std::ops::RangeInclusive<usize>,
) -> Vec<Heading> {
    let mut out = Vec::new();
    for index in 0..paragraph_count(doc) {
        if skip.contains(&index) {
            continue;
        }
        let Some(paragraph) = paragraph_ref(doc, index) else {
            continue;
        };
        let Some(level) = outline_level(doc, &paragraph.props) else {
            continue;
        };
        if level < levels.0 || level > levels.1 {
            continue;
        }
        let text: String = plain_text(paragraph)
            .chars()
            .filter(|c| *c != OBJECT_CHAR)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let text = text.trim().to_string();
        if !text.is_empty() {
            out.push(Heading {
                paragraph: index,
                level,
                text,
            });
        }
    }
    out
}

/// Закладка `_Toc…` абзаца `index`: уже стоящая или новая на весь абзац.
fn toc_bookmark(doc: &mut Document, index: usize, taken: &mut Vec<String>) -> String {
    let existing = paragraph_ref(doc, index).and_then(|p| {
        p.content.iter().find_map(|inline| match inline {
            Inline::BookmarkStart(b) if b.name.starts_with("_Toc") => Some(b.name.clone()),
            _ => None,
        })
    });
    if let Some(name) = existing {
        return name;
    }
    let mut number = 100_000_000 + taken.len();
    let name = loop {
        let name = format!("_Toc{number}");
        if !taken.contains(&name) {
            break name;
        }
        number += 1;
    };
    taken.push(name.clone());
    let len = paragraph_ref(doc, index).map_or(0, |p| plain_text(p).len());
    add_bookmark(
        doc,
        &name,
        Position::new(index, 0),
        Position::new(index, len),
    );
    name
}

/// Имена всех закладок документа.
fn bookmark_names(doc: &Document) -> Vec<String> {
    crate::links::bookmarks(doc)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// Фрагмент с оформлением `props` и одним элементом.
fn run(props: &RunProps, item: RunContent) -> Inline {
    Inline::Run(Run {
        props: props.clone(),
        content: vec![item],
    })
}

/// Знак поля.
fn field_char(props: &RunProps, kind: FieldCharKind) -> Inline {
    run(props, RunContent::FieldChar(FieldChar::new(kind)))
}

/// Строка оглавления: текст заголовка, табуляция и номер страницы
/// (поле PAGEREF), всё — ссылкой на закладку, если `link`.
fn entry(heading: &Heading, bookmark: &str, page: usize, link: bool) -> Vec<Inline> {
    let props = RunProps {
        link: link.then(|| Hyperlink::to_bookmark(bookmark)),
        ..RunProps::default()
    };
    vec![
        run(&props, RunContent::Text(heading.text.clone())),
        run(&props, RunContent::Tab),
        field_char(&props, FieldCharKind::Begin),
        run(
            &props,
            RunContent::FieldCode(format!(" PAGEREF {bookmark} \\h ")),
        ),
        field_char(&props, FieldCharKind::Separate),
        run(&props, RunContent::Text(page.to_string())),
        field_char(&props, FieldCharKind::End),
    ]
}

/// Стиль строки оглавления уровня `level`, как в Word: «toc N» с отступом
/// слева на 0,39 см на уровень и интервалом 5 пт после.
fn toc_style(doc: &mut Document, level: u8) -> String {
    let id = format!("TOC{level}");
    let name = format!("toc {level}");
    ensure_style(doc, &id, &name, StyleKind::Paragraph, |style| {
        style.paragraph.spacing = Spacing {
            after: Some(Twips(100)),
            ..Spacing::default()
        };
        if level > 1 {
            style.paragraph.indentation.left = Some(Twips(220 * (i32::from(level) - 1)));
        }
    })
}

/// Элементы абзаца по одному, с оформлением фрагмента.
enum Piece {
    Other(Inline),
    Item(RunProps, RunContent),
}

fn pieces(paragraph: &Paragraph) -> Vec<Piece> {
    let mut out = Vec::new();
    for inline in &paragraph.content {
        match inline {
            Inline::Run(run) => out.extend(
                run.content
                    .iter()
                    .map(|item| Piece::Item(run.props.clone(), item.clone())),
            ),
            other => out.push(Piece::Other(other.clone())),
        }
    }
    out
}

fn inlines(pieces: impl IntoIterator<Item = Piece>) -> Vec<Inline> {
    pieces
        .into_iter()
        .map(|piece| match piece {
            Piece::Other(inline) => inline,
            Piece::Item(props, item) => run(&props, item),
        })
        .collect()
}

/// Начало поля TOC в его первом абзаце: что стоит до знака начала
/// (другие поля, текст) и оформление знака начала. Знак начала — последний
/// знак начала поля верхнего уровня в абзаце.
fn head(paragraph: &Paragraph) -> (Vec<Inline>, RunProps) {
    let pieces = pieces(paragraph);
    let mut depth = 0usize;
    let mut at = None;
    for (k, piece) in pieces.iter().enumerate() {
        if let Piece::Item(_, RunContent::FieldChar(c)) = piece {
            match c.kind {
                FieldCharKind::Begin => {
                    if depth == 0 {
                        at = Some(k);
                    }
                    depth += 1;
                }
                FieldCharKind::End => depth = depth.saturating_sub(1),
                FieldCharKind::Separate => {}
            }
        }
    }
    let at = at.unwrap_or(pieces.len());
    let props = match pieces.get(at) {
        Some(Piece::Item(props, _)) => props.clone(),
        _ => RunProps::default(),
    };
    (inlines(pieces.into_iter().take(at)), props)
}

/// Конец поля TOC в его последнем абзаце: оформление знака конца и что
/// стоит после него.
fn tail(paragraph: &Paragraph, same: bool) -> (RunProps, Vec<Inline>) {
    let pieces = pieces(paragraph);
    // В абзаце, где поле и началось, считаем от его знака начала.
    let mut depth = usize::from(!same);
    let mut at = None;
    for (k, piece) in pieces.iter().enumerate() {
        if let Piece::Item(_, RunContent::FieldChar(c)) = piece {
            match c.kind {
                FieldCharKind::Begin => depth += 1,
                FieldCharKind::End => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        at = Some(k);
                    }
                }
                FieldCharKind::Separate => {}
            }
        }
    }
    let at = at.unwrap_or(pieces.len());
    let props = match pieces.get(at) {
        Some(Piece::Item(props, _)) => props.clone(),
        _ => RunProps::default(),
    };
    (props, inlines(pieces.into_iter().skip(at + 1)))
}

/// Перестраивает оглавления, которые задевают абзацы с `first` по `last`.
/// Номера страниц заголовков берутся из `context` и сдвигаются вместе
/// с абзацами. Возвращает, сколько оглавлений перестроено, и новые
/// `last` и номера страниц абзацев.
pub fn update_tocs(
    doc: &mut Document,
    first: usize,
    mut last: usize,
    context: &FieldContext,
) -> (usize, usize, Vec<usize>) {
    let mut pages = context.page_of_paragraph.clone();
    let mut done = 0;
    let mut skip = 0;
    while let Some(toc) = toc_fields(doc)
        .into_iter()
        .filter(|t| t.last >= first && t.first <= last)
        .nth(skip)
    {
        let before = paragraph_count(doc);
        if let Some(count) = rebuild(doc, &toc, &pages, context.russian) {
            // Абзацы оглавления — на странице его начала.
            let page = pages.get(toc.first).copied().unwrap_or(1);
            let end = (toc.last + 1).min(pages.len());
            let start = toc.first.min(end);
            pages.splice(start..end, std::iter::repeat_n(page, count));
            last = (last + paragraph_count(doc)).saturating_sub(before);
            done += 1;
        }
        skip += 1;
    }
    (done, last, pages)
}

/// Перестраивает оглавление `toc`. Возвращает, сколько в нём теперь
/// абзацев; `None` — оглавление не верхнего уровня или разорвано
/// таблицей, его не трогаем.
fn rebuild(doc: &mut Document, toc: &TocField, pages: &[usize], russian: bool) -> Option<usize> {
    let (section, start) = top_level(doc, toc.first)?;
    let (end_section, end) = top_level(doc, toc.last)?;
    if section != end_section || end - start != toc.last - toc.first {
        return None;
    }
    let first = paragraph_ref(doc, toc.first)?.clone();
    let last = paragraph_ref(doc, toc.last)?.clone();
    let (prefix, begin_props) = head(&first);
    let (end_props, suffix) = tail(&last, toc.first == toc.last);

    let found = headings(doc, levels(&toc.code), &(toc.first..=toc.last));
    let link = has_switch(&toc.code, "\\h");
    let width = crate::table::text_width(doc, toc.first);
    let mut taken = bookmark_names(doc);
    let mut paragraphs = Vec::new();
    for heading in &found {
        let bookmark = toc_bookmark(doc, heading.paragraph, &mut taken);
        let page = pages.get(heading.paragraph).copied().unwrap_or(1);
        let style = toc_style(doc, heading.level);
        let mut paragraph = Paragraph {
            props: ParagraphProps {
                style: Some(style),
                tabs: vec![TabStop {
                    position: Twips(width),
                    kind: TabStopKind::Right,
                    leader: TabLeader::Dot,
                }],
                ..ParagraphProps::default()
            },
            content: entry(heading, &bookmark, page, link),
        };
        tidy(&mut paragraph);
        paragraphs.push(paragraph);
    }
    if paragraphs.is_empty() {
        let text = if russian {
            "Элементы оглавления не найдены."
        } else {
            "No table of contents entries found."
        };
        paragraphs.push(Paragraph::from_runs([Run::text(text)]));
    }
    // Начало поля — перед первой строкой.
    let mut opening = prefix;
    opening.push(field_char(&begin_props, FieldCharKind::Begin));
    opening.push(run(&begin_props, RunContent::FieldCode(toc.code.clone())));
    opening.push(field_char(&begin_props, FieldCharKind::Separate));
    if let Some(head) = paragraphs.first_mut() {
        opening.append(&mut head.content);
        head.content = opening;
        tidy(head);
    }
    // Конец поля — в своём абзаце, как у Word.
    let mut closing = Paragraph {
        props: last.props.clone(),
        content: vec![field_char(&end_props, FieldCharKind::End)],
    };
    closing.content.extend(suffix);
    tidy(&mut closing);
    paragraphs.push(closing);

    let count = paragraphs.len();
    let blocks = &mut doc.sections.get_mut(section)?.blocks;
    blocks.splice(start..=end, paragraphs.into_iter().map(Block::Paragraph));
    Some(count)
}

/// Вставляет оглавление у места `at`: заголовок «Оглавление» и поле TOC
/// с заголовками 1–3 уровней. Абзац с курсором делится, если курсор не
/// в его начале. Возвращает место сразу после оглавления.
pub fn insert_toc(doc: &mut Document, at: Position, context: &FieldContext) -> Position {
    let mut at = at;
    if at.offset > 0 {
        at = split_paragraph(doc, at);
    }
    let Some((section, block)) = top_level(doc, at.paragraph) else {
        return at;
    };
    // В ячейке таблицы оглавление не ставим.
    if crate::table::in_table(doc, at.paragraph) {
        return at;
    }
    let title_style = ensure_style(
        doc,
        "TOCHeading",
        "TOC Heading",
        StyleKind::Paragraph,
        |style| {
            style.paragraph.unknown = vec![RawXml::new(r#"<w:outlineLvl w:val="9"/>"#)];
            style.paragraph.spacing = Spacing {
                before: Some(Twips(240)),
                after: Some(Twips(120)),
                line: Some(LineSpacing::Multiple(259)),
            };
            style.run.size = Some(letter_model::HalfPoints(32));
        },
    );
    let heading_style = doc
        .styles
        .styles
        .iter()
        .find(|s| heading_number(s) == Some(1))
        .map(|s| s.id.clone());
    if let Some(style) = doc
        .styles
        .styles
        .iter_mut()
        .find(|s| s.id == title_style && s.based_on.is_none())
    {
        style.based_on = heading_style;
    }
    let title = if context.russian {
        "Оглавление"
    } else {
        "Contents"
    };
    let mut heading = Paragraph::from_runs([Run::text(title)]);
    heading.props.style = Some(title_style);
    let none = RunProps::default();
    let opening = Paragraph {
        props: ParagraphProps::default(),
        content: vec![
            field_char(&none, FieldCharKind::Begin),
            run(&none, RunContent::FieldCode(TOC_CODE.to_string())),
            field_char(&none, FieldCharKind::Separate),
        ],
    };
    let closing = Paragraph {
        props: ParagraphProps::default(),
        content: vec![field_char(&none, FieldCharKind::End)],
    };
    let Some(target) = doc.sections.get_mut(section) else {
        return at;
    };
    target.blocks.splice(
        block..block,
        [heading.into(), opening.into(), closing.into()],
    );
    // Новые абзацы — на странице абзаца с курсором.
    let mut context = context.clone();
    let page = context
        .page_of_paragraph
        .get(at.paragraph)
        .copied()
        .unwrap_or(1);
    let index = at.paragraph.min(context.page_of_paragraph.len());
    context
        .page_of_paragraph
        .splice(index..index, std::iter::repeat_n(page, 3));
    let before = paragraph_count(doc);
    update_tocs(doc, at.paragraph + 1, at.paragraph + 2, &context);
    let added = paragraph_count(doc) + 3 - before;
    let after = Position::new(at.paragraph + added, 0);
    if paragraph_mut(doc, after.paragraph).is_some() {
        after
    } else {
        Position::new(paragraph_count(doc).saturating_sub(1), 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::plain_text;
    use letter_model::fields;

    /// Документ: заголовки 1 и 2 уровня и текст.
    fn doc() -> Document {
        let mut doc = Document::new();
        for (id, name) in [("Heading1", "heading 1"), ("Heading2", "heading 2")] {
            let mut style = Style::new(id, StyleKind::Paragraph);
            style.name = Some(name.into());
            doc.styles.styles.push(style);
        }
        let heading = |text: &str, style: &str| {
            let mut p = Paragraph::text(text);
            p.props.style = Some(style.into());
            Block::from(p)
        };
        doc.sections[0].blocks = vec![
            Paragraph::text("Intro").into(),
            heading("Chapter", "Heading1"),
            Paragraph::text("Body").into(),
            heading("Part", "Heading2"),
        ];
        doc
    }

    fn context(pages: Vec<usize>) -> FieldContext {
        FieldContext {
            page_of_paragraph: pages,
            russian: true,
            ..FieldContext::default()
        }
    }

    fn texts(doc: &Document) -> Vec<String> {
        (0..paragraph_count(doc))
            .filter_map(|i| paragraph_ref(doc, i))
            .map(|p| plain_text(p).replace(OBJECT_CHAR, ""))
            .collect()
    }

    #[test]
    fn levels_come_from_styles_and_outline() {
        let mut doc = doc();
        let p = |doc: &Document, i| paragraph_ref(doc, i).cloned().unwrap_or_default();
        assert_eq!(outline_level(&doc, &p(&doc, 1).props), Some(1));
        assert_eq!(outline_level(&doc, &p(&doc, 3).props), Some(2));
        assert_eq!(outline_level(&doc, &p(&doc, 2).props), None);
        let mut body = p(&doc, 2);
        body.props.unknown = vec![RawXml::new(r#"<w:outlineLvl w:val="2"/>"#)];
        assert_eq!(outline_level(&doc, &body.props), Some(3));
        // Стиль на основе заголовка — тоже заголовок.
        let mut style = Style::new("Mine", StyleKind::Paragraph);
        style.based_on = Some("Heading2".into());
        doc.styles.styles.push(style);
        body.props = ParagraphProps {
            style: Some("Mine".into()),
            ..ParagraphProps::default()
        };
        assert_eq!(outline_level(&doc, &body.props), Some(2));
        assert_eq!(levels(TOC_CODE), (1, 3));
        assert_eq!(levels(" TOC \\h "), (1, 9));
    }

    #[test]
    fn toc_is_inserted_and_updated() {
        let mut doc = doc();
        let at = insert_toc(&mut doc, Position::new(0, 0), &context(vec![1, 1, 2, 3]));
        let lines = texts(&doc);
        assert_eq!(
            lines,
            [
                "Оглавление",
                "Chapter\t1",
                "Part\t3",
                "",
                "Intro",
                "Chapter",
                "Body",
                "Part"
            ]
        );
        assert_eq!(at, Position::new(4, 0));
        // Строки — стилей «Оглавление 1» и «Оглавление 2», со ссылками на
        // закладки заголовков.
        let line = paragraph_ref(&doc, 2).cloned().unwrap_or_default();
        assert_eq!(line.props.style.as_deref(), Some("TOC2"));
        assert!(doc.styles.get("TOC2").is_some());
        let link = line.runs().find_map(|r| r.props.link.clone());
        let anchor = link.and_then(|l| l.anchor).unwrap_or_default();
        assert!(anchor.starts_with("_Toc"));
        assert!(crate::links::bookmark_position(&doc, &anchor).is_some());
        let page_field = fields(&line);
        assert_eq!(page_field.len(), 1);
        assert!(page_field[0].code.contains("PAGEREF"));
        // Заголовок изменился — обновление перестраивает строки, закладки
        // остаются прежние.
        let Some(paragraph) = paragraph_mut(&mut doc, 7) else {
            panic!("нет заголовка");
        };
        paragraph.content.push(Inline::Run(Run::text(" two")));
        let pages = context(vec![1; 8]);
        let (count, last, _) = update_tocs(&mut doc, 0, 7, &pages);
        assert_eq!((count, last), (1, 7));
        assert_eq!(texts(&doc)[2], "Part two\t1");
        let again = paragraph_ref(&doc, 2).cloned().unwrap_or_default();
        let link = again.runs().find_map(|r| r.props.link.clone());
        assert_eq!(link.and_then(|l| l.anchor), Some(anchor));
    }

    #[test]
    fn toc_is_deleted_with_its_title() {
        let mut doc = doc();
        let before = doc.plain_text();
        insert_toc(&mut doc, Position::new(0, 0), &context(vec![1, 1, 2, 3]));
        assert_eq!(toc_fields(&doc).len(), 1);
        // Курсор после оглавления: удаляется первое (единственное).
        let last = paragraph_count(&doc) - 1;
        assert!(delete_toc(&mut doc, last));
        assert!(toc_fields(&doc).is_empty());
        // Остаются только скрытые закладки заголовков, как в Word.
        assert_eq!(doc.plain_text(), before);
        assert!(!delete_toc(&mut doc, 0));
    }

    #[test]
    fn empty_toc_says_so() {
        let mut doc = Document::new();
        doc.sections[0].blocks = vec![Paragraph::text("Text").into()];
        insert_toc(&mut doc, Position::new(0, 0), &context(vec![1]));
        assert_eq!(texts(&doc)[1], "Элементы оглавления не найдены.");
    }
}
