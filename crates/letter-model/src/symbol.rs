//! Знаки из шрифтов-символов (Symbol, Wingdings): `w:sym` в тексте
//! и маркеры списков.

/// Знак из шрифта (`w:sym`): Word хранит шрифт и номер знака в нём,
/// а не букву Юникода. В тексте абзаца — знак [`crate::OBJECT_CHAR`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol {
    /// Шрифт, например `Wingdings`.
    pub font: String,
    /// Номер знака в шрифте (у Symbol и Wingdings — в области
    /// `U+F020`–`U+F0FF`).
    pub code: u32,
}

impl Symbol {
    /// Знак Юникода, которым показывается символ: у шрифтов Symbol и
    /// Wingdings — похожий знак (самих шрифтов обычно нет), у других —
    /// знак с этим номером.
    pub fn unicode(&self) -> char {
        let c = char::from_u32(self.code).unwrap_or('\u{fffd}');
        if is_symbol_font(&self.font) {
            symbol_char(&self.font, c)
        } else {
            c
        }
    }
}

/// Шрифт со знаками на месте букв (Symbol, Wingdings).
pub fn is_symbol_font(family: &str) -> bool {
    let family = family.to_lowercase();
    family == "symbol" || family.starts_with("wingdings")
}

/// Знак Юникода, похожий на знак `c` шрифта `family` (Symbol или
/// Wingdings; у них знаки лежат в области `U+F000`–`U+F0FF` или
/// `U+0020`–`U+00FF`). Знак другого шрифта не меняется; неизвестный знак
/// шрифта-символа становится маркером «•».
pub fn symbol_char(family: &str, c: char) -> char {
    let family = family.to_lowercase();
    let wingdings = family.starts_with("wingdings");
    if family != "symbol" && !wingdings {
        return c;
    }
    let code = c as u32;
    let byte = if (0xF020..=0xF0FF).contains(&code) {
        code - 0xF000
    } else if (0x20..=0xFF).contains(&code) {
        code
    } else {
        return c;
    };
    if wingdings {
        return match byte {
            0x6C => '●',
            0x6E => '■',
            0x71 => '❑',
            0x76 => '❖',
            0x9F => '•',
            0xA1 => '○',
            0xA7 => '▪',
            0xA8 => '◻',
            0xD8 | 0xE0 => '➢',
            0xE7 => '←',
            0xE8 => '→',
            0xE9 => '↑',
            0xEA => '↓',
            0xFB => '✗',
            0xFC => '✓',
            0xFD => '☒',
            0xFE => '☑',
            0x4A => '☺',
            0x4C => '☹',
            0x28 => '☎',
            0x2A => '✉',
            _ => '•',
        };
    }
    // Шрифт Symbol: греческие буквы на месте латинских и знаки математики.
    const UPPER: &str = "ΑΒΧΔΕΦΓΗΙϑΚΛΜΝΟΠΘΡΣΤΥςΩΞΨΖ";
    const LOWER: &str = "αβχδεφγηιϕκλμνοπθρστυϖωξψζ";
    match byte {
        0x41..=0x5A => UPPER.chars().nth((byte - 0x41) as usize).unwrap_or('•'),
        0x61..=0x7A => LOWER.chars().nth((byte - 0x61) as usize).unwrap_or('•'),
        0x20..=0x40 | 0x5B..=0x60 | 0x7B..=0x7E => match byte {
            0x22 => '∀',
            0x24 => '∃',
            0x27 => '∋',
            0x2A => '∗',
            0x2D => '−',
            0x40 => '≅',
            0x5C => '∴',
            0x5E => '⊥',
            0x60 => '‾',
            0x7E => '∼',
            _ => char::from_u32(byte).unwrap_or('•'),
        },
        0xA5 => '∞',
        0xA7 => '♣',
        0xA8 => '♦',
        0xA9 => '♥',
        0xAA => '♠',
        0xAB => '↔',
        0xAC => '←',
        0xAD => '↑',
        0xAE => '→',
        0xAF => '↓',
        0xB0 => '°',
        0xB1 => '±',
        0xB4 => '×',
        0xB5 => '∝',
        0xB6 => '∂',
        0xB7 => '•',
        0xB8 => '÷',
        0xB9 => '≠',
        0xBA => '≡',
        0xBB => '≈',
        0xBC => '…',
        0xA3 => '≤',
        0xB3 => '≥',
        0xC5 => '⊕',
        0xC6 => '∅',
        0xC7 => '∩',
        0xC8 => '∪',
        0xCE => '∈',
        0xD1 => '∇',
        0xD2 => '®',
        0xD3 => '©',
        0xD4 => '™',
        0xD5 => '∏',
        0xD6 => '√',
        0xD7 => '⋅',
        0xD8 => '¬',
        0xD9 => '∧',
        0xDA => '∨',
        0xDB => '⇔',
        0xDE => '⇒',
        0xE5 => '∑',
        0xF2 => '∫',
        _ => '•',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_fonts_map_to_unicode() {
        assert_eq!(symbol_char("Symbol", '\u{f0b7}'), '•');
        assert_eq!(symbol_char("Symbol", '\u{f061}'), 'α');
        assert_eq!(symbol_char("Symbol", '\u{f057}'), 'Ω');
        assert_eq!(symbol_char("Wingdings", '\u{f0a7}'), '▪');
        assert_eq!(symbol_char("Wingdings 2", '\u{f0fc}'), '✓');
        assert_eq!(symbol_char("Courier New", 'o'), 'o');
        assert!(is_symbol_font("Wingdings 3"));
        assert!(!is_symbol_font("Arial"));
        let sym = |font: &str, code| Symbol {
            font: font.into(),
            code,
        };
        assert_eq!(sym("Symbol", 0xF070).unicode(), 'π');
        assert_eq!(sym("Times New Roman", 0x2014).unicode(), '—');
    }
}
