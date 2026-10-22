//! Чтение RTF в модель документа.
//!
//! Оформление в RTF задаётся «состоянием»: управляющие слова меняют
//! текущий шрифт, размер, выравнивание и т. п., а группа `{…}` по
//! выходу возвращает состояние, каким оно было при входе. Текст
//! получает оформление, действующее в момент, когда он встретился.
//!
//! Оформление абзацев и фрагментов из RTF записывается в модель
//! полностью и явно (все поля `Some`), как его понимает Word: стиль
//! абзаца в RTF — только ссылка, само оформление всегда повторено
//! у текста. Стили читаются из `\stylesheet` и сохраняются со своим
//! оформлением, чтобы при сохранении в .docx их можно было применить.

use letter_model::{
    Alignment, Block, BreakKind, Color, Document, FirstLineIndent, HalfPoints, Highlight,
    Indentation, Inline, LineSpacing, Orientation, PageMargins, PageSize, Paragraph,
    ParagraphProps, Run, RunContent, RunFonts, RunProps, Section, SectionProps, Spacing, Style,
    StyleKind, TabLeader, TabStop, TabStopKind, Table, TableCell, TableRow, Twips, Underline,
    VerticalAlign,
};

use crate::ReadError;
use crate::codepage::{SYMBOL_CHARSET, charset_codepage, decode, decode_symbol};
use crate::tokens::{Token, Tokenizer};

/// Шрифт по умолчанию, если в файле нет таблицы шрифтов.
const DEFAULT_FONT: &str = "Times New Roman";

/// Размер шрифта по умолчанию в RTF: 12 пт.
const DEFAULT_SIZE: u32 = 24;

/// Глубже этого вложенность групп не отслеживается (защита от
/// специально испорченных файлов).
const MAX_DEPTH: usize = 512;

/// Куда идёт текст в текущей группе.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dest {
    /// Текст документа.
    Body,
    /// Группа пропускается целиком.
    Skip,
    /// Таблица шрифтов.
    FontTable,
    /// Таблица цветов.
    ColorTable,
    /// Таблица стилей.
    StyleSheet,
    /// Сведения о документе (`\info`): читаются только отдельные поля.
    InfoGroup,
    /// Одно поле сведений о документе.
    Info(InfoField),
    /// Оформление символов по умолчанию (`\*\defchp`).
    DefaultChars,
    /// Оформление абзацев по умолчанию (`\*\defpap`).
    DefaultParagraph,
}

/// Поле сведений о документе (`\info`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfoField {
    Title,
    Subject,
    Author,
    Keywords,
    Comment,
    Operator,
    Category,
}

/// Оформление символов в текущем состоянии.
#[derive(Debug, Clone, PartialEq)]
struct Chars {
    font: Option<i32>,
    size: u32,
    bold: bool,
    italic: bool,
    underline: Underline,
    strike: bool,
    double_strike: bool,
    color: i32,
    highlight: i32,
    vertical_align: VerticalAlign,
    hidden: bool,
    style: Option<i32>,
}

impl Default for Chars {
    fn default() -> Chars {
        Chars {
            font: None,
            size: DEFAULT_SIZE,
            bold: false,
            italic: false,
            underline: Underline::None,
            strike: false,
            double_strike: false,
            color: 0,
            highlight: 0,
            vertical_align: VerticalAlign::Baseline,
            hidden: false,
            style: None,
        }
    }
}

/// Оформление абзаца в текущем состоянии.
#[derive(Debug, Clone, PartialEq)]
struct Para {
    style: Option<i32>,
    alignment: Alignment,
    left: i32,
    right: i32,
    first: i32,
    before: i32,
    after: i32,
    line: i32,
    line_multiple: bool,
    keep_next: bool,
    keep_lines: bool,
    page_break_before: bool,
    widow_control: bool,
    contextual: bool,
    tabs: Vec<TabStop>,
    tab_kind: TabStopKind,
    tab_leader: TabLeader,
    in_table: bool,
}

impl Default for Para {
    fn default() -> Para {
        Para {
            style: None,
            alignment: Alignment::Left,
            left: 0,
            right: 0,
            first: 0,
            before: 0,
            after: 0,
            line: 0,
            line_multiple: false,
            keep_next: false,
            keep_lines: false,
            page_break_before: false,
            widow_control: false,
            contextual: false,
            tabs: Vec::new(),
            tab_kind: TabStopKind::Left,
            tab_leader: TabLeader::None,
            in_table: false,
        }
    }
}

/// Параметры страницы.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Page {
    width: i32,
    height: i32,
    left: i32,
    right: i32,
    top: i32,
    bottom: i32,
    header: i32,
    footer: i32,
    gutter: i32,
    landscape: bool,
}

impl Default for Page {
    /// Значения RTF по умолчанию: лист Letter, поля 1 и 1,25 дюйма.
    fn default() -> Page {
        Page {
            width: 12240,
            height: 15840,
            left: 1800,
            right: 1800,
            top: 1440,
            bottom: 1440,
            header: 720,
            footer: 720,
            gutter: 0,
            landscape: false,
        }
    }
}

/// Состояние, которое сохраняется при входе в группу.
#[derive(Debug, Clone)]
struct State {
    dest: Dest,
    chars: Chars,
    para: Para,
    /// Сколько знаков замены идёт после `\uN`.
    uc: usize,
    /// Следующая группа с `\*` пропускается, если её слово незнакомо.
    starred: bool,
    /// В группе ещё не было ни одного слова (первое слово группы
    /// определяет её назначение).
    fresh: bool,
}

/// Шрифт из таблицы шрифтов.
#[derive(Debug, Clone, Default)]
struct Font {
    name: String,
    charset: i32,
}

/// Стиль, который читается из таблицы стилей.
#[derive(Debug, Clone, Default)]
struct StyleEntry {
    number: Option<i32>,
    kind: Option<StyleKind>,
    based_on: Option<i32>,
    next: Option<i32>,
    name: String,
}

/// Читатель RTF.
struct Reader {
    state: State,
    stack: Vec<State>,
    /// Сколько знаков ещё пропустить после `\uN`.
    skip: usize,
    /// Первая половина суррогатной пары после `\uN`.
    high_surrogate: Option<u16>,
    codepage: u16,
    default_font: i32,
    fonts: Vec<(i32, Font)>,
    font: Font,
    font_number: Option<i32>,
    colors: Vec<Color>,
    rgb: [u8; 3],
    rgb_set: bool,
    styles: Vec<(StyleEntry, Chars, Para)>,
    style: StyleEntry,
    /// Глубина группы таблицы шрифтов или стилей: её прямые подгруппы —
    /// отдельные шрифты или стили.
    table_level: usize,
    /// С последнего конца абзаца было `\pard` или текст: в конце
    /// документа это ещё один абзац.
    paragraph_pending: bool,
    info: Vec<(InfoField, String)>,
    default_chars: Option<Chars>,
    default_para: Option<Para>,
    doc_page: Page,
    page: Page,
    default_tab: Option<i32>,
    html_spacing: bool,
    sections: Vec<Section>,
    blocks: Vec<Block>,
    runs: Vec<Run>,
    /// В абзаце что-то было (текст или его оформление важно).
    table: Option<Table>,
    row: Vec<TableCell>,
    cell: Vec<Block>,
}

/// Читает документ RTF.
pub fn read(data: &[u8]) -> Result<Document, ReadError> {
    let start = data
        .iter()
        .position(|b| !b.is_ascii_whitespace() && *b != 0xEF && *b != 0xBB && *b != 0xBF)
        .unwrap_or(data.len());
    if !data[start..].starts_with(b"{\\rtf") {
        return Err(ReadError::NotRtf);
    }
    let mut reader = Reader::new();
    let mut tokens = Tokenizer::new(&data[start..]);
    while let Some(token) = tokens.next_token() {
        reader.token(token);
        if reader.stack.is_empty() && reader.state.dest == Dest::Skip {
            break;
        }
    }
    Ok(reader.finish())
}

impl Reader {
    fn new() -> Reader {
        Reader {
            state: State {
                dest: Dest::Body,
                chars: Chars::default(),
                para: Para::default(),
                uc: 1,
                starred: false,
                fresh: true,
            },
            stack: Vec::new(),
            skip: 0,
            high_surrogate: None,
            codepage: 1252,
            default_font: 0,
            fonts: Vec::new(),
            font: Font::default(),
            font_number: None,
            colors: Vec::new(),
            rgb: [0; 3],
            rgb_set: false,
            styles: Vec::new(),
            style: StyleEntry::default(),
            table_level: 0,
            paragraph_pending: false,
            info: Vec::new(),
            default_chars: None,
            default_para: None,
            doc_page: Page::default(),
            page: Page::default(),
            default_tab: None,
            html_spacing: false,
            sections: Vec::new(),
            blocks: Vec::new(),
            runs: Vec::new(),
            table: None,
            row: Vec::new(),
            cell: Vec::new(),
        }
    }

    fn token(&mut self, token: Token<'_>) {
        match token {
            Token::Open => self.open(),
            Token::Close => self.close(),
            Token::Control { name, param } => {
                if self.skip > 0 {
                    self.skip -= 1;
                    return;
                }
                let fresh = std::mem::replace(&mut self.state.fresh, false);
                if name == "*" {
                    self.state.starred = true;
                    self.state.fresh = fresh;
                    return;
                }
                if self.state.dest == Dest::Skip {
                    return;
                }
                if fresh && self.destination(name) {
                    return;
                }
                let table_style =
                    self.state.dest == Dest::StyleSheet && matches!(name, "cs" | "ts" | "ds");
                if fresh && self.state.starred && !table_style {
                    // Незнакомая группа с `\*` — её можно пропустить.
                    self.state.dest = Dest::Skip;
                    return;
                }
                self.control(name, param);
            }
            Token::Bytes(mut bytes) => {
                if self.skip > 0 {
                    let n = self.skip.min(bytes.len());
                    bytes.drain(..n);
                    self.skip -= n;
                }
                self.state.fresh = false;
                if !bytes.is_empty() {
                    let text = self.decode(&bytes);
                    self.text(&text);
                }
            }
            Token::Binary(_) => self.skip = self.skip.saturating_sub(1),
        }
    }

    fn open(&mut self) {
        self.skip = 0;
        let mut inner = self.state.clone();
        inner.starred = false;
        inner.fresh = true;
        if self.stack.len() >= MAX_DEPTH {
            inner.dest = Dest::Skip;
        }
        self.stack.push(std::mem::replace(&mut self.state, inner));
        let entry = self.stack.len() == self.table_level + 1;
        match self.state.dest {
            Dest::FontTable if entry => {
                self.font = Font::default();
                self.font_number = None;
            }
            Dest::StyleSheet if entry => {
                self.style = StyleEntry::default();
                self.state.chars = Chars::default();
                self.state.para = Para::default();
            }
            _ => {}
        }
    }

    fn close(&mut self) {
        self.skip = 0;
        let empty = self.blocks.is_empty() && self.table.is_none() && self.sections.is_empty();
        if self.stack.len() == 1
            && self.state.dest == Dest::Body
            && (self.paragraph_pending || empty)
        {
            // Последний абзац без `\par` (или единственный пустой): его
            // оформление — внутри группы документа, забираем его до её
            // закрытия.
            self.end_paragraph(false);
        }
        let Some(outer) = self.stack.pop() else {
            // Лишняя `}` — конец документа.
            self.state.dest = Dest::Skip;
            return;
        };
        let inner = std::mem::replace(&mut self.state, outer);
        let entry = self.stack.len() == self.table_level;
        match inner.dest {
            Dest::FontTable if entry && self.state.dest == Dest::FontTable => self.finish_font(),
            Dest::StyleSheet if entry && self.state.dest == Dest::StyleSheet => {
                self.finish_style(inner.chars, inner.para)
            }
            Dest::DefaultChars if self.state.dest != Dest::DefaultChars => {
                self.default_chars = Some(inner.chars);
            }
            Dest::DefaultParagraph if self.state.dest != Dest::DefaultParagraph => {
                self.default_para = Some(inner.para);
            }
            _ => {}
        }
        if self.stack.is_empty() && self.state.dest == Dest::Body && inner.dest == Dest::Body {
            // Закрылась группа всего документа.
            self.state.dest = Dest::Skip;
        }
    }

    /// Слово, которое начинает группу и задаёт, куда идёт её текст.
    /// `true`, если слово обработано.
    fn destination(&mut self, name: &str) -> bool {
        let dest = match name {
            "fonttbl" => {
                self.table_level = self.stack.len();
                Dest::FontTable
            }
            "colortbl" => {
                self.colors.clear();
                Dest::ColorTable
            }
            "stylesheet" => {
                self.table_level = self.stack.len();
                Dest::StyleSheet
            }
            "info" => Dest::InfoGroup,
            "defchp" => {
                self.state.chars = Chars::default();
                Dest::DefaultChars
            }
            "defpap" => {
                self.state.para = Para::default();
                Dest::DefaultParagraph
            }
            "title" | "subject" | "author" | "keywords" | "doccomm" | "operator" | "category"
                if self.state.dest == Dest::InfoGroup =>
            {
                let field = match name {
                    "title" => InfoField::Title,
                    "subject" => InfoField::Subject,
                    "author" => InfoField::Author,
                    "keywords" => InfoField::Keywords,
                    "doccomm" => InfoField::Comment,
                    "operator" => InfoField::Operator,
                    _ => InfoField::Category,
                };
                self.info.push((field, String::new()));
                Dest::Info(field)
            }
            // Картинки, колонтитулы, сноски, примечания, фигуры, списки,
            // служебные таблицы — пока не поддерживаются.
            "pict" | "header" | "headerl" | "headerr" | "headerf" | "footer" | "footerl"
            | "footerr" | "footerf" | "footnote" | "annotation" | "atnid" | "atnauthor" | "shp"
            | "shpinst" | "nonshppict" | "shppict" | "object" | "objdata" | "objclass"
            | "listtable" | "listoverridetable" | "revtbl" | "rsidtbl" | "filetbl" | "xmlnstbl"
            | "mmathPr" | "themedata" | "colorschememapping" | "datastore" | "latentstyles"
            | "generator" | "pgdsctbl" | "fldinst" | "xe" | "tc" | "tcn" | "bkmkstart"
            | "bkmkend" | "template" | "userprops" | "docvar" | "pnseclvl" | "pn"
            | "listpicture" | "fchars" | "lchars" | "nesttableprops" | "pgptbl"
            | "wgrffmtfilter" | "ftnsep" | "ftnsepc" | "ftncn" | "aftnsep" | "aftnsepc"
            | "aftncn" | "background" | "passwordhash" | "protusertbl" | "ud" => Dest::Skip,
            _ => return false,
        };
        self.state.dest = dest;
        true
    }

    /// Обычное управляющее слово.
    fn control(&mut self, name: &str, param: Option<i32>) {
        let value = param.unwrap_or(1);
        let on = param != Some(0);
        match self.state.dest {
            Dest::Skip | Dest::Info(_) | Dest::InfoGroup => return,
            Dest::FontTable => {
                match name {
                    "f" => self.font_number = Some(value),
                    "fcharset" => self.font.charset = value,
                    _ => {}
                }
                return;
            }
            Dest::ColorTable => {
                let channel = u8::try_from(value.clamp(0, 255)).unwrap_or(0);
                match name {
                    "red" => self.rgb[0] = channel,
                    "green" => self.rgb[1] = channel,
                    "blue" => self.rgb[2] = channel,
                    _ => return,
                }
                self.rgb_set = true;
                return;
            }
            Dest::StyleSheet => match name {
                "s" => {
                    self.style.number = Some(value);
                    self.style.kind = Some(StyleKind::Paragraph);
                    return;
                }
                "cs" => {
                    self.style.number = Some(value);
                    self.style.kind = Some(StyleKind::Character);
                    return;
                }
                "ts" | "ds" => {
                    self.style.number = Some(value);
                    self.style.kind = Some(StyleKind::Table);
                    return;
                }
                "sbasedon" => {
                    self.style.based_on = Some(value);
                    return;
                }
                "snext" => {
                    self.style.next = Some(value);
                    return;
                }
                _ => {}
            },
            _ => {}
        }

        if self.chars_control(name, param, value, on) || self.para_control(name, param, value, on) {
            return;
        }
        match name {
            "ansicpg" => self.codepage = u16::try_from(value).unwrap_or(1252),
            "deff" => self.default_font = value,
            "deftab" => self.default_tab = Some(value),
            "htmautsp" => self.html_spacing = true,
            "uc" => self.state.uc = usize::try_from(value).unwrap_or(1),
            "u" => self.unicode(value),
            "par" => self.end_paragraph(false),
            "cell" | "nestcell" => self.end_cell(),
            "row" | "nestrow" => self.end_row(),
            "sect" => self.end_section(),
            "sectd" => self.page = self.doc_page,
            "tab" => self.push(RunContent::Tab),
            "line" => self.push(RunContent::Break(BreakKind::Line)),
            "page" => self.push(RunContent::Break(BreakKind::Page)),
            "column" => self.push(RunContent::Break(BreakKind::Column)),
            "paperw" => {
                self.doc_page.width = value;
                self.page.width = value;
            }
            "paperh" => {
                self.doc_page.height = value;
                self.page.height = value;
            }
            "margl" => {
                self.doc_page.left = value;
                self.page.left = value;
            }
            "margr" => {
                self.doc_page.right = value;
                self.page.right = value;
            }
            "margt" => {
                self.doc_page.top = value;
                self.page.top = value;
            }
            "margb" => {
                self.doc_page.bottom = value;
                self.page.bottom = value;
            }
            "gutter" => {
                self.doc_page.gutter = value;
                self.page.gutter = value;
            }
            "landscape" => {
                self.doc_page.landscape = true;
                self.page.landscape = true;
            }
            "pgwsxn" => self.page.width = value,
            "pghsxn" => self.page.height = value,
            "marglsxn" => self.page.left = value,
            "margrsxn" => self.page.right = value,
            "margtsxn" => self.page.top = value,
            "margbsxn" => self.page.bottom = value,
            "guttersxn" => self.page.gutter = value,
            "headery" => self.page.header = value,
            "footery" => self.page.footer = value,
            "lndscpsxn" => self.page.landscape = on,
            _ => {
                if let Some(c) = symbol(name) {
                    self.text(c);
                }
            }
        }
    }

    /// Оформление символов. `true`, если слово про него.
    fn chars_control(&mut self, name: &str, param: Option<i32>, value: i32, on: bool) -> bool {
        let chars = &mut self.state.chars;
        match name {
            "plain" => {
                *chars = Chars::default();
            }
            "f" => chars.font = Some(value),
            "fs" => chars.size = u32::try_from(value).unwrap_or(DEFAULT_SIZE).max(1),
            "b" => chars.bold = on,
            "i" => chars.italic = on,
            "strike" => chars.strike = on,
            "striked" => chars.double_strike = on,
            "v" => chars.hidden = on,
            "cf" => chars.color = value,
            "highlight" => chars.highlight = value,
            "super" => chars.vertical_align = VerticalAlign::Superscript,
            "sub" => chars.vertical_align = VerticalAlign::Subscript,
            "nosupersub" => chars.vertical_align = VerticalAlign::Baseline,
            "cs" => chars.style = Some(value),
            "ulnone" => chars.underline = Underline::None,
            _ => match underline(name) {
                Some(kind) => {
                    chars.underline = if param == Some(0) {
                        Underline::None
                    } else {
                        kind
                    };
                }
                None => return false,
            },
        }
        true
    }

    /// Оформление абзаца. `true`, если слово про него.
    fn para_control(&mut self, name: &str, _param: Option<i32>, value: i32, on: bool) -> bool {
        let para = &mut self.state.para;
        match name {
            "pard" => {
                *para = Para::default();
                self.paragraph_pending = self.state.dest == Dest::Body;
            }
            "s" => para.style = Some(value),
            "ql" => para.alignment = Alignment::Left,
            "qc" => para.alignment = Alignment::Center,
            "qr" => para.alignment = Alignment::Right,
            "qj" => para.alignment = Alignment::Justify,
            "qd" => para.alignment = Alignment::Distribute,
            "li" | "lin" => para.left = value,
            "ri" | "rin" => para.right = value,
            "fi" => para.first = value,
            "sb" => para.before = value,
            "sa" => para.after = value,
            "sl" => para.line = value,
            "slmult" => para.line_multiple = on,
            "keepn" => para.keep_next = on,
            "keep" => para.keep_lines = on,
            "pagebb" => para.page_break_before = on,
            "widctlpar" => para.widow_control = true,
            "nowidctlpar" => para.widow_control = false,
            "contextualspace" => para.contextual = on,
            "intbl" => para.in_table = true,
            "itap" => para.in_table = value > 0,
            "tqr" => para.tab_kind = TabStopKind::Right,
            "tqc" => para.tab_kind = TabStopKind::Center,
            "tqdec" => para.tab_kind = TabStopKind::Decimal,
            "tldot" => para.tab_leader = TabLeader::Dot,
            "tlmdot" => para.tab_leader = TabLeader::MiddleDot,
            "tlhyph" => para.tab_leader = TabLeader::Hyphen,
            "tlul" => para.tab_leader = TabLeader::Underscore,
            "tlth" => para.tab_leader = TabLeader::Heavy,
            "tx" | "tb" => {
                let kind = if name == "tb" {
                    TabStopKind::Bar
                } else {
                    para.tab_kind
                };
                para.tabs.push(TabStop {
                    position: Twips(value),
                    kind,
                    leader: para.tab_leader,
                });
                para.tab_kind = TabStopKind::Left;
                para.tab_leader = TabLeader::None;
            }
            _ => return false,
        }
        true
    }

    /// Знак по номеру Unicode (`\uN`, число со знаком).
    fn unicode(&mut self, value: i32) {
        let code = if value < 0 { value + 65536 } else { value };
        let code = u16::try_from(code.clamp(0, 0xFFFF)).unwrap_or(0xFFFD);
        self.skip = self.state.uc;
        let text = match (self.high_surrogate.take(), code) {
            (_, 0xD800..=0xDBFF) => {
                self.high_surrogate = Some(code);
                return;
            }
            (Some(high), 0xDC00..=0xDFFF) => String::from_utf16_lossy(&[high, code]),
            (_, code) => String::from_utf16_lossy(&[code]),
        };
        self.text(&text);
    }

    fn decode(&self, bytes: &[u8]) -> String {
        let font = self.current_font();
        if font.is_some_and(|f| f.charset == SYMBOL_CHARSET) {
            return decode_symbol(bytes);
        }
        let codepage = font
            .and_then(|f| charset_codepage(f.charset))
            .unwrap_or(self.codepage);
        decode(bytes, codepage)
    }

    fn current_font(&self) -> Option<&Font> {
        let number = self.state.chars.font.unwrap_or(self.default_font);
        self.fonts
            .iter()
            .find(|(n, _)| *n == number)
            .map(|(_, f)| f)
    }

    /// Текст в текущее место.
    fn text(&mut self, text: &str) {
        match self.state.dest {
            Dest::Body => {
                if !self.state.chars.hidden {
                    self.paragraph_pending = true;
                    self.push_text(text);
                }
            }
            Dest::FontTable => {
                for c in text.chars() {
                    if c == ';' {
                        self.finish_font();
                    } else {
                        self.font.name.push(c);
                    }
                }
            }
            Dest::ColorTable => {
                for c in text.chars() {
                    if c == ';' {
                        let color = if self.rgb_set {
                            Color::Rgb(self.rgb[0], self.rgb[1], self.rgb[2])
                        } else {
                            Color::Auto
                        };
                        self.colors.push(color);
                        self.rgb = [0; 3];
                        self.rgb_set = false;
                    }
                }
            }
            Dest::StyleSheet => self.style.name.push_str(text),
            Dest::Info(field) => {
                if let Some((f, value)) = self.info.last_mut()
                    && *f == field
                {
                    value.push_str(text);
                }
            }
            Dest::Skip | Dest::InfoGroup | Dest::DefaultChars | Dest::DefaultParagraph => {}
        }
    }

    fn finish_font(&mut self) {
        let font = std::mem::take(&mut self.font);
        if let Some(number) = self.font_number.take() {
            let name = font.name.trim().to_owned();
            self.fonts.retain(|(n, _)| *n != number);
            self.fonts.push((number, Font { name, ..font }));
        }
    }

    fn finish_style(&mut self, chars: Chars, para: Para) {
        let mut entry = std::mem::take(&mut self.style);
        entry.name = entry.name.trim().trim_end_matches(';').trim().to_owned();
        if entry.kind.is_none() {
            // `{\stylesheet{Normal;}}`: стиль без номера — «Обычный» (`\s0`).
            entry.kind = Some(StyleKind::Paragraph);
            entry.number = Some(0);
        }
        if !entry.name.is_empty() {
            self.styles.push((entry, chars, para));
        }
    }

    fn push_text(&mut self, text: &str) {
        let props = self.run_props();
        match self.runs.last_mut() {
            Some(run) if run.props == props => match run.content.last_mut() {
                Some(RunContent::Text(t)) => t.push_str(text),
                _ => run.content.push(RunContent::Text(text.to_owned())),
            },
            _ => self.runs.push(Run {
                props,
                content: vec![RunContent::Text(text.to_owned())],
            }),
        }
    }

    fn push(&mut self, content: RunContent) {
        if self.state.dest != Dest::Body {
            return;
        }
        self.paragraph_pending = true;
        let props = self.run_props();
        match self.runs.last_mut() {
            Some(run) if run.props == props => run.content.push(content),
            _ => self.runs.push(Run {
                props,
                content: vec![content],
            }),
        }
    }

    fn font_name(&self, number: i32) -> String {
        self.fonts
            .iter()
            .find(|(n, _)| *n == number)
            .map(|(_, f)| f.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| DEFAULT_FONT.to_owned())
    }

    fn color(&self, index: i32) -> Color {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.colors.get(i))
            .copied()
            .unwrap_or(Color::Auto)
    }

    fn style_id(&self, number: i32, kind: StyleKind) -> Option<String> {
        self.styles
            .iter()
            .find(|(e, _, _)| e.number == Some(number) && e.kind == Some(kind))
            .map(|(e, _, _)| style_id(&e.name))
    }

    /// Оформление текущего текста — полностью, как в RTF.
    fn run_props(&self) -> RunProps {
        let chars = &self.state.chars;
        RunProps {
            style: chars
                .style
                .and_then(|n| self.style_id(n, StyleKind::Character)),
            fonts: RunFonts::new(self.font_name(chars.font.unwrap_or(self.default_font))),
            size: Some(HalfPoints(chars.size)),
            bold: Some(chars.bold),
            italic: Some(chars.italic),
            underline: Some(chars.underline),
            strike: Some(chars.strike),
            double_strike: Some(chars.double_strike),
            color: Some(self.color(chars.color)),
            theme_color: None,
            highlight: Some(highlight(self.color(chars.highlight), chars.highlight)),
            vertical_align: Some(chars.vertical_align),
            ..RunProps::default()
        }
    }

    /// Оформление текущего абзаца — полностью, как в RTF.
    fn paragraph_props(&self) -> ParagraphProps {
        let para = &self.state.para;
        ParagraphProps {
            // Абзац без `\s` — в стиле 0 («Обычный»).
            style: self.style_id(para.style.unwrap_or(0), StyleKind::Paragraph),
            alignment: Some(para.alignment),
            indentation: Indentation {
                left: Some(Twips(para.left)),
                right: Some(Twips(para.right)),
                first_line: Some(first_line(para.first)),
            },
            spacing: Spacing {
                before: Some(Twips(para.before)),
                after: Some(Twips(para.after)),
                line: Some(line_spacing(para.line, para.line_multiple)),
            },
            contextual_spacing: Some(para.contextual),
            keep_next: Some(para.keep_next),
            keep_lines: Some(para.keep_lines),
            page_break_before: Some(para.page_break_before),
            widow_control: Some(para.widow_control),
            tabs: para.tabs.clone(),
            unknown: Vec::new(),
        }
    }

    /// Конец абзаца (`\par` или конец ячейки).
    fn end_paragraph(&mut self, cell_end: bool) {
        if self.state.dest != Dest::Body {
            return;
        }
        self.paragraph_pending = false;
        let paragraph = Paragraph {
            props: self.paragraph_props(),
            content: std::mem::take(&mut self.runs)
                .into_iter()
                .map(Inline::Run)
                .collect(),
        };
        if self.state.para.in_table || cell_end {
            self.cell.push(paragraph.into());
        } else {
            self.flush_table();
            self.blocks.push(paragraph.into());
        }
    }

    fn end_cell(&mut self) {
        if self.state.dest != Dest::Body {
            return;
        }
        self.end_paragraph(true);
        let blocks = std::mem::take(&mut self.cell);
        self.row.push(TableCell {
            raw_props: Vec::new(),
            blocks,
        });
    }

    fn end_row(&mut self) {
        if self.state.dest != Dest::Body {
            return;
        }
        if !self.cell.is_empty() {
            // Текст после последней `\cell` — ещё одна ячейка.
            let blocks = std::mem::take(&mut self.cell);
            self.row.push(TableCell {
                raw_props: Vec::new(),
                blocks,
            });
        }
        let cells = std::mem::take(&mut self.row);
        self.table
            .get_or_insert_with(Table::default)
            .rows
            .push(TableRow {
                raw_props: Vec::new(),
                cells,
            });
    }

    fn flush_table(&mut self) {
        if !self.row.is_empty() || !self.cell.is_empty() {
            self.end_row();
        }
        if let Some(table) = self.table.take() {
            self.blocks.push(table.into());
        }
    }

    fn end_section(&mut self) {
        if self.state.dest != Dest::Body {
            return;
        }
        if self.paragraph_pending {
            self.end_paragraph(false);
        }
        self.flush_table();
        let blocks = std::mem::take(&mut self.blocks);
        self.sections.push(Section {
            props: section_props(self.page),
            blocks,
        });
    }

    fn finish(mut self) -> Document {
        while !self.stack.is_empty() {
            self.close();
        }
        self.state.dest = Dest::Body;
        // Текст после последнего `\par` — тоже абзац (Word так и пишет:
        // последний абзац без `\par`).
        if self.paragraph_pending || (self.blocks.is_empty() && self.table.is_none()) {
            self.end_paragraph(false);
        }
        self.flush_table();
        let blocks = std::mem::take(&mut self.blocks);
        if !blocks.is_empty() || self.sections.is_empty() {
            self.sections.push(Section {
                props: section_props(self.page),
                blocks,
            });
        }
        let mut doc = Document::new();
        doc.sections = std::mem::take(&mut self.sections);
        if let Some(tab) = self.default_tab.filter(|&t| t > 0) {
            doc.settings.default_tab_stop = Twips(tab);
        }
        doc.settings.add_paragraph_spacing = !self.html_spacing;
        self.fill_styles(&mut doc);
        for (field, value) in std::mem::take(&mut self.info) {
            let core = &mut doc.properties.core;
            let slot = match field {
                InfoField::Title => &mut core.title,
                InfoField::Subject => &mut core.subject,
                InfoField::Author => &mut core.creator,
                InfoField::Keywords => &mut core.keywords,
                InfoField::Comment => &mut core.description,
                InfoField::Operator => &mut core.last_modified_by,
                InfoField::Category => &mut core.category,
            };
            *slot = Some(value);
        }
        doc
    }

    /// Таблица стилей и оформление по умолчанию.
    fn fill_styles(&self, doc: &mut Document) {
        let defaults = &mut doc.styles.defaults;
        let default_chars = self.default_chars.clone().unwrap_or_default();
        defaults.run = self.chars_diff(&default_chars);
        defaults.run.fonts =
            RunFonts::new(self.font_name(default_chars.font.unwrap_or(self.default_font)));
        defaults.run.size = Some(HalfPoints(default_chars.size));
        defaults.paragraph = self.para_diff(&self.default_para.clone().unwrap_or_default());

        for (entry, chars, para) in &self.styles {
            let Some(kind) = entry.kind.filter(|k| *k != StyleKind::Table) else {
                continue;
            };
            let mut style = Style::new(style_id(&entry.name), kind);
            style.name = Some(entry.name.clone());
            style.is_default = match kind {
                StyleKind::Paragraph => entry.number == Some(0),
                _ => entry.name.eq_ignore_ascii_case("Default Paragraph Font"),
            };
            style.based_on = entry
                .based_on
                .filter(|&n| entry.number != Some(n))
                .and_then(|n| self.style_id(n, kind));
            style.next = entry.next.and_then(|n| self.style_id(n, kind));
            style.run = self.chars_diff(chars);
            if kind == StyleKind::Paragraph {
                style.paragraph = self.para_diff(para);
            }
            if doc.styles.get(&style.id).is_none() {
                doc.styles.styles.push(style);
            }
        }
    }

    /// Оформление символов, отличающееся от значений по умолчанию
    /// (для стилей: заданное явно).
    fn chars_diff(&self, chars: &Chars) -> RunProps {
        let base = Chars::default();
        let some = |differs: bool| differs.then_some(());
        RunProps {
            style: None,
            fonts: chars
                .font
                .map(|n| RunFonts::new(self.font_name(n)))
                .unwrap_or_default(),
            size: some(chars.size != base.size).map(|()| HalfPoints(chars.size)),
            bold: some(chars.bold).map(|()| true),
            italic: some(chars.italic).map(|()| true),
            underline: some(chars.underline != base.underline).map(|()| chars.underline),
            strike: some(chars.strike).map(|()| true),
            double_strike: some(chars.double_strike).map(|()| true),
            color: some(chars.color != 0).map(|()| self.color(chars.color)),
            theme_color: None,
            highlight: some(chars.highlight != 0)
                .map(|()| highlight(self.color(chars.highlight), chars.highlight)),
            vertical_align: some(chars.vertical_align != base.vertical_align)
                .map(|()| chars.vertical_align),
            ..RunProps::default()
        }
    }

    /// Оформление абзаца, отличающееся от значений по умолчанию.
    fn para_diff(&self, para: &Para) -> ParagraphProps {
        let base = Para::default();
        ParagraphProps {
            style: None,
            alignment: (para.alignment != base.alignment).then_some(para.alignment),
            indentation: Indentation {
                left: (para.left != 0).then_some(Twips(para.left)),
                right: (para.right != 0).then_some(Twips(para.right)),
                first_line: (para.first != 0).then(|| first_line(para.first)),
            },
            spacing: Spacing {
                before: (para.before != 0).then_some(Twips(para.before)),
                after: (para.after != 0).then_some(Twips(para.after)),
                line: (para.line != 0).then(|| line_spacing(para.line, para.line_multiple)),
            },
            contextual_spacing: para.contextual.then_some(true),
            keep_next: para.keep_next.then_some(true),
            keep_lines: para.keep_lines.then_some(true),
            page_break_before: para.page_break_before.then_some(true),
            widow_control: para.widow_control.then_some(true),
            tabs: para.tabs.clone(),
            unknown: Vec::new(),
        }
    }
}

/// Вид подчёркивания по управляющему слову.
fn underline(name: &str) -> Option<Underline> {
    let kind = match name {
        "ul" => Underline::Single,
        "ulw" => Underline::Words,
        "uldb" => Underline::Double,
        "ulth" => Underline::Thick,
        "uld" => Underline::Dotted,
        "ulthd" => Underline::DottedHeavy,
        "uldash" => Underline::Dash,
        "ulthdash" => Underline::DashedHeavy,
        "ulldash" => Underline::DashLong,
        "ulthldash" => Underline::DashLongHeavy,
        "uldashd" => Underline::DotDash,
        "ulthdashd" => Underline::DashDotHeavy,
        "uldashdd" => Underline::DotDotDash,
        "ulthdashdd" => Underline::DashDotDotHeavy,
        "ulwave" => Underline::Wave,
        "ulhwave" => Underline::WavyHeavy,
        "ululdbwave" => Underline::WavyDouble,
        _ => return None,
    };
    Some(kind)
}

/// Управляющее слово-знак: `\emdash`, `\~` и т. п.
fn symbol(name: &str) -> Option<&'static str> {
    let text = match name {
        "\\" => "\\",
        "{" => "{",
        "}" => "}",
        "~" => "\u{a0}",
        "-" => "\u{ad}",
        "_" => "\u{2011}",
        "emdash" => "—",
        "endash" => "–",
        "emspace" => "\u{2003}",
        "enspace" => "\u{2002}",
        "qmspace" => "\u{2005}",
        "bullet" => "•",
        "lquote" => "‘",
        "rquote" => "’",
        "ldblquote" => "“",
        "rdblquote" => "”",
        "zwj" => "\u{200d}",
        "zwnj" => "\u{200c}",
        "zwbo" => "\u{200b}",
        "ltrmark" => "\u{200e}",
        "rtlmark" => "\u{200f}",
        _ => return None,
    };
    Some(text)
}

/// Отступ первой строки: положительный — отступ, отрицательный — выступ.
fn first_line(first: i32) -> FirstLineIndent {
    if first < 0 {
        FirstLineIndent::Hanging(Twips(first.saturating_neg()))
    } else {
        FirstLineIndent::FirstLine(Twips(first))
    }
}

/// Межстрочный интервал из `\slN` и `\slmultN`.
fn line_spacing(line: i32, multiple: bool) -> LineSpacing {
    match line {
        0 => LineSpacing::SINGLE,
        n if multiple => LineSpacing::Multiple(n.unsigned_abs()),
        n if n < 0 => LineSpacing::Exact(Twips(n.saturating_neg())),
        n => LineSpacing::AtLeast(Twips(n)),
    }
}

/// Цвет маркера: в RTF — номер цвета в таблице, в Word — один из 16.
fn highlight(color: Color, index: i32) -> Highlight {
    let Color::Rgb(r, g, b) = color else {
        return Highlight::None;
    };
    if index == 0 {
        return Highlight::None;
    }
    const ALL: [Highlight; 16] = [
        Highlight::Black,
        Highlight::Blue,
        Highlight::Cyan,
        Highlight::Green,
        Highlight::Magenta,
        Highlight::Red,
        Highlight::Yellow,
        Highlight::White,
        Highlight::DarkBlue,
        Highlight::DarkCyan,
        Highlight::DarkGreen,
        Highlight::DarkMagenta,
        Highlight::DarkRed,
        Highlight::DarkYellow,
        Highlight::DarkGray,
        Highlight::LightGray,
    ];
    let distance = |h: &Highlight| {
        let (hr, hg, hb) = h.rgb().unwrap_or((0, 0, 0));
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(r, hr) + d(g, hg) + d(b, hb)
    };
    ALL.iter()
        .min_by_key(|h| distance(h))
        .copied()
        .unwrap_or(Highlight::None)
}

/// Идентификатор стиля по имени, как его делает Word: слова с большой
/// буквы, без пробелов («heading 1» → «Heading1»).
pub fn style_id(name: &str) -> String {
    name.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

fn section_props(page: Page) -> SectionProps {
    SectionProps {
        page_size: Some(PageSize {
            width: Twips(page.width),
            height: Twips(page.height),
            orientation: if page.landscape {
                Orientation::Landscape
            } else {
                Orientation::Portrait
            },
        }),
        margins: Some(PageMargins {
            top: Twips(page.top),
            right: Twips(page.right),
            bottom: Twips(page.bottom),
            left: Twips(page.left),
            header: Twips(page.header),
            footer: Twips(page.footer),
            gutter: Twips(page.gutter),
        }),
        unknown: Vec::new(),
    }
}
