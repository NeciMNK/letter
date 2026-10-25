//! Сборка модели документа из текста .doc.

use std::collections::HashMap;

use letter_model::{
    Block, BreakKind, Document, FieldChar, FieldCharKind, Inline, Paragraph, ParagraphProps, Run,
    RunContent, RunProps, Section,
};

use crate::ReadError;
use crate::fib::{Fib, Table, u32_at};
use crate::fkp::{Span, chpx, find, papx};
use crate::sprm::{Names, apply_paragraph, apply_run, sprms};
use crate::styles::{fonts, style_sheet};
use crate::text::{pieces, text};

/// Концы разделов (CP после последнего знака каждого раздела) из
/// `PlcfSed`.
fn section_ends(plc: &[u8]) -> Vec<u32> {
    // (n + 1) CP и n описаний раздела по 12 байт.
    if plc.len() < 4 + 16 {
        return Vec::new();
    }
    let count = (plc.len() - 4) / 16;
    (1..=count).filter_map(|i| u32_at(plc, 4 * i)).collect()
}

/// Абзацы и разделы по мере чтения текста.
struct Builder {
    sections: Vec<Section>,
    blocks: Vec<Block>,
    paragraph: Paragraph,
    /// Набираемый текст текущего фрагмента и его свойства.
    pending: Vec<RunContent>,
    props: RunProps,
    text: String,
    /// Поля: внутри кода поля (до разделителя) текст — код.
    fields: Vec<bool>,
}

impl Builder {
    fn new() -> Builder {
        Builder {
            sections: Vec::new(),
            blocks: Vec::new(),
            paragraph: Paragraph::default(),
            pending: Vec::new(),
            props: RunProps::default(),
            text: String::new(),
            fields: Vec::new(),
        }
    }

    /// В коде поля (а не в его результате).
    fn in_code(&self) -> bool {
        self.fields.last() == Some(&true)
    }

    fn flush_text(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.text);
        self.pending.push(if self.in_code() {
            RunContent::FieldCode(text)
        } else {
            RunContent::Text(text)
        });
    }

    fn push(&mut self, item: RunContent) {
        self.flush_text();
        self.pending.push(item);
    }

    /// Закрывает фрагмент: дальше текст с другими свойствами.
    fn end_run(&mut self) {
        self.flush_text();
        if !self.pending.is_empty() {
            let content = std::mem::take(&mut self.pending);
            self.paragraph.content.push(Inline::Run(Run {
                props: self.props.clone(),
                content,
            }));
        }
    }

    /// Свойства следующего текста.
    fn set_props(&mut self, props: &RunProps) {
        if *props != self.props {
            self.end_run();
            self.props = props.clone();
        }
    }

    fn end_paragraph(&mut self, props: ParagraphProps) {
        self.end_run();
        let mut paragraph = std::mem::take(&mut self.paragraph);
        paragraph.props = props;
        self.blocks.push(paragraph.into());
    }

    fn end_section(&mut self) {
        let mut blocks = std::mem::take(&mut self.blocks);
        if blocks.is_empty() {
            blocks.push(Paragraph::default().into());
        }
        self.sections.push(Section {
            props: Default::default(),
            blocks,
        });
    }

    fn field(&mut self, kind: FieldCharKind) {
        self.flush_text();
        match kind {
            FieldCharKind::Begin => {
                self.pending
                    .push(RunContent::FieldChar(FieldChar::new(kind)));
                self.fields.push(true);
            }
            FieldCharKind::Separate => {
                if let Some(code) = self.fields.last_mut() {
                    *code = false;
                    self.pending
                        .push(RunContent::FieldChar(FieldChar::new(kind)));
                }
            }
            FieldCharKind::End => {
                if self.fields.pop().is_some() {
                    self.pending
                        .push(RunContent::FieldChar(FieldChar::new(kind)));
                }
            }
        }
    }
}

/// Оформление по местам в потоке: отрезки и разобранные свойства.
struct Formatting<'a> {
    chpx: Vec<Span<'a>>,
    papx: Vec<Span<'a>>,
    names: Names<'a>,
    /// Разобранные свойства знаков по номеру отрезка.
    runs: HashMap<usize, RunProps>,
}

impl Formatting<'_> {
    /// Свойства знака в месте `fc`.
    fn run(&mut self, fc: u32) -> RunProps {
        let i = self.chpx.partition_point(|s| s.fc_end <= fc);
        if self.chpx.get(i).is_none_or(|s| s.fc_start > fc) {
            return RunProps::default();
        }
        if let Some(props) = self.runs.get(&i) {
            return props.clone();
        }
        let mut props = RunProps::default();
        apply_run(&mut props, &sprms(self.chpx[i].grpprl), &self.names);
        self.runs.insert(i, props.clone());
        props
    }

    /// Свойства абзаца, чей знак конца — в месте `fc`.
    fn paragraph(&self, fc: u32) -> ParagraphProps {
        let mut props = ParagraphProps::default();
        if let Some(span) = find(&self.papx, fc) {
            props.style = self.names.style(span.istd).filter(|id| id != "Normal");
            apply_paragraph(&mut props, &sprms(span.grpprl));
        }
        props
    }
}

/// Собирает документ.
pub(crate) fn build(fib: &Fib, word: &[u8], table: &[u8]) -> Result<Document, ReadError> {
    let pieces = pieces(fib.table(table, Table::Clx))?;
    let ends = section_ends(fib.table(table, Table::PlcfSed));
    let fonts = fonts(fib.table(table, Table::SttbfFfn));
    let sheet = style_sheet(fib.table(table, Table::Stshf), &fonts);
    let mut formatting = Formatting {
        chpx: chpx(fib.table(table, Table::PlcfBteChpx), word),
        papx: papx(fib.table(table, Table::PlcfBtePapx), word),
        names: Names {
            fonts: &fonts,
            styles: &sheet.ids,
        },
        runs: HashMap::new(),
    };
    let units = text(&pieces, word, 0, fib.ccp_text);
    let mut builder = Builder::new();
    let mut previous = 0u16;
    let mut i = 0;
    while i < units.len() {
        let (cp, fc, unit) = units[i];
        i += 1;
        let props = formatting.run(fc);
        builder.set_props(&props);
        match unit {
            0x0D => builder.end_paragraph(formatting.paragraph(fc)),
            // Конец ячейки; второй подряд — конец строки таблицы.
            0x07 if previous == 0x07 => {}
            0x07 => builder.end_paragraph(formatting.paragraph(fc)),
            0x0C if ends.contains(&(cp + 1)) => {
                builder.end_paragraph(formatting.paragraph(fc));
                builder.end_section();
            }
            0x0C => builder.push(RunContent::Break(BreakKind::Page)),
            0x0E => builder.push(RunContent::Break(BreakKind::Column)),
            0x0B => builder.push(RunContent::Break(BreakKind::Line)),
            0x09 => builder.push(RunContent::Tab),
            0x13 => builder.field(FieldCharKind::Begin),
            0x14 => builder.field(FieldCharKind::Separate),
            0x15 => builder.field(FieldCharKind::End),
            0x1E => builder.text.push('\u{2011}'),
            0x1F => builder.text.push('\u{AD}'),
            // Значки рисунков, сносок, примечаний и прочие служебные знаки.
            0x00..=0x1F => {}
            // Знак вне основной плоскости — пара суррогатов.
            0xD800..=0xDBFF => {
                let low = units.get(i).map(|u| u.2);
                if let Some(low @ 0xDC00..=0xDFFF) = low {
                    i += 1;
                    let code =
                        0x10000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(low) - 0xDC00);
                    builder.text.extend(char::from_u32(code));
                }
            }
            _ => builder.text.extend(char::from_u32(u32::from(unit))),
        }
        previous = unit;
    }
    // Последний знак текста — конец абзаца; если нет, абзац всё равно
    // закрывается.
    if !builder.pending.is_empty() || !builder.text.is_empty() {
        builder.end_paragraph(ParagraphProps::default());
    }
    builder.end_section();
    let mut doc = Document::new();
    doc.sections = builder.sections;
    doc.styles = sheet.styles;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_ends_from_plc() {
        let mut plc = Vec::new();
        for cp in [0u32, 10, 25] {
            plc.extend(cp.to_le_bytes());
        }
        plc.extend([0u8; 24]);
        assert_eq!(section_ends(&plc), [10, 25]);
        assert!(section_ends(&[0; 8]).is_empty());
    }
}
