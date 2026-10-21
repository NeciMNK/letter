//! Позиция в документе — место между двумя буквами, где стоит курсор.

/// К какой строке относится позиция на месте переноса: конец строки
/// и начало следующей — одно место в тексте, но два места на экране.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Affinity {
    /// К тексту после позиции: курсор в начале следующей строки.
    #[default]
    Downstream,
    /// К тексту перед позицией: курсор в конце строки (так бывает после
    /// щелчка правее конца строки или по клавише End).
    Upstream,
}

/// Позиция курсора в документе.
///
/// Абзац задаётся номером по порядку во всём документе (абзацы в ячейках
/// таблиц тоже считаются, как [`letter_layout::PageLine::paragraph`]),
/// место в абзаце — смещением в байтах в тексте абзаца, всегда на границе
/// графем (буква с ударением или эмодзи из нескольких кодов — одна
/// графема, курсор не встаёт внутрь). Табуляция, разрыв строки и разрыв
/// страницы — по одному знаку текста (`\t`, `\n`, `\u{c}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Position {
    /// Номер абзаца.
    pub paragraph: usize,
    /// Смещение в тексте абзаца, байты.
    pub offset: usize,
    /// Строка на месте переноса.
    pub affinity: Affinity,
}

impl Position {
    /// Позиция `offset` в абзаце `paragraph`, к тексту после неё.
    pub fn new(paragraph: usize, offset: usize) -> Position {
        Position {
            paragraph,
            offset,
            affinity: Affinity::Downstream,
        }
    }

    /// Та же позиция, но с другой привязкой к строке.
    pub fn with_affinity(self, affinity: Affinity) -> Position {
        Position { affinity, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_start_downstream() {
        let p = Position::new(2, 5);
        assert_eq!(p.affinity, Affinity::Downstream);
        assert_eq!(Position::default(), Position::new(0, 0));
        let up = p.with_affinity(Affinity::Upstream);
        assert_eq!((up.paragraph, up.offset), (2, 5));
        assert_ne!(up, p);
    }
}
