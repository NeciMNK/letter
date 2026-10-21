//! Разбор потока RTF на лексемы: скобки групп, управляющие слова,
//! байты текста.
//!
//! RTF — текстовый формат из семибитных символов: `{` и `}` открывают
//! и закрывают группу, `\слово` или `\слово123` — управляющее слово
//! с необязательным числом, `\'hh` — байт текста в шестнадцатеричном
//! виде, остальное — сам текст. Переводы строк в файле ничего не значат.

/// Лексема RTF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token<'a> {
    /// `{`.
    Open,
    /// `}`.
    Close,
    /// Управляющее слово (`\b`, `\fs24`, `\u-3913`) или символ (`\~`,
    /// `\*`, `\{`). У символа имя — сам символ.
    Control {
        /// Имя без обратной косой черты.
        name: &'a str,
        /// Число после имени.
        param: Option<i32>,
    },
    /// Байты текста: обычные знаки и `\'hh` подряд. Кодировка — та,
    /// что у текущего шрифта.
    Bytes(Vec<u8>),
    /// Двоичные данные после `\binN` (их пропускают).
    Binary(&'a [u8]),
}

/// Читатель лексем.
pub struct Tokenizer<'a> {
    data: &'a [u8],
    pos: usize,
}

/// Наибольшая длина имени управляющего слова по спецификации.
const MAX_NAME: usize = 32;

impl<'a> Tokenizer<'a> {
    /// Читатель с начала `data`.
    pub fn new(data: &'a [u8]) -> Tokenizer<'a> {
        Tokenizer { data, pos: 0 }
    }

    /// Следующая лексема. `None` — конец данных.
    pub fn next_token(&mut self) -> Option<Token<'a>> {
        loop {
            let &byte = self.data.get(self.pos)?;
            match byte {
                b'{' => {
                    self.pos += 1;
                    return Some(Token::Open);
                }
                b'}' => {
                    self.pos += 1;
                    return Some(Token::Close);
                }
                b'\r' | b'\n' | 0 => self.pos += 1,
                b'\\' => {
                    if let Some(token) = self.control() {
                        return Some(token);
                    }
                }
                _ => return Some(self.text()),
            }
        }
    }

    /// Обычный текст и `\'hh` до следующей скобки или управляющего слова.
    fn text(&mut self) -> Token<'a> {
        let mut bytes = Vec::new();
        while let Some(&byte) = self.data.get(self.pos) {
            match byte {
                b'{' | b'}' => break,
                b'\r' | b'\n' | 0 => self.pos += 1,
                b'\\' => {
                    if self.data.get(self.pos + 1) == Some(&b'\'')
                        && let Some(value) = self.hex_byte(self.pos + 2)
                    {
                        bytes.push(value);
                        self.pos += 4;
                    } else {
                        break;
                    }
                }
                _ => {
                    bytes.push(byte);
                    self.pos += 1;
                }
            }
        }
        Token::Bytes(bytes)
    }

    fn hex_byte(&self, at: usize) -> Option<u8> {
        let digits = self.data.get(at..at + 2)?;
        let text = std::str::from_utf8(digits).ok()?;
        u8::from_str_radix(text, 16).ok()
    }

    /// Управляющее слово или символ; `self.pos` — на обратной черте.
    /// `None`, если лексема ничего не дала (обрыв в конце файла).
    fn control(&mut self) -> Option<Token<'a>> {
        let start = self.pos + 1;
        let Some(&first) = self.data.get(start) else {
            self.pos = start;
            return None;
        };
        if first == b'\'' {
            // `\'hh` — байт текста.
            return Some(self.text_from_hex());
        }
        if !first.is_ascii_alphabetic() {
            // Управляющий символ: `\~`, `\-`, `\*`, `\{`, `\\`, `\<перевод строки>`.
            self.pos = start + 1;
            let name = std::str::from_utf8(&self.data[start..start + 1]).unwrap_or("?");
            let name = match first {
                b'\r' | b'\n' => "par",
                _ => name,
            };
            return Some(Token::Control { name, param: None });
        }
        let mut end = start;
        while end < self.data.len()
            && end - start < MAX_NAME
            && self.data[end].is_ascii_alphabetic()
        {
            end += 1;
        }
        let name = std::str::from_utf8(&self.data[start..end]).unwrap_or("");
        let mut pos = end;
        let negative = self.data.get(pos) == Some(&b'-');
        if negative {
            pos += 1;
        }
        let digits_start = pos;
        while pos < self.data.len() && self.data[pos].is_ascii_digit() {
            pos += 1;
        }
        let param = if pos > digits_start {
            // Больше десяти цифр в число не помещается: хватит первых.
            let digits_end = pos.min(digits_start + 10);
            let digits = std::str::from_utf8(&self.data[digits_start..digits_end]).unwrap_or("0");
            let value = digits.parse::<i64>().unwrap_or(0);
            let value = if negative { -value } else { value };
            Some(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
        } else {
            if negative {
                pos -= 1;
            }
            None
        };
        // Пробел после слова — разделитель, он не текст.
        if self.data.get(pos) == Some(&b' ') {
            pos += 1;
        }
        self.pos = pos;
        if name == "bin" {
            let length = usize::try_from(param.unwrap_or(0)).unwrap_or(0);
            let end = (self.pos + length).min(self.data.len());
            let data = &self.data[self.pos..end];
            self.pos = end;
            return Some(Token::Binary(data));
        }
        Some(Token::Control { name, param })
    }

    fn text_from_hex(&mut self) -> Token<'a> {
        match self.hex_byte(self.pos + 2) {
            Some(_) => self.text(),
            None => {
                // Испорченный `\'`: пропускаем его.
                self.pos = (self.pos + 2).min(self.data.len());
                Token::Bytes(Vec::new())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(data: &str) -> Vec<Token<'_>> {
        let mut tokenizer = Tokenizer::new(data.as_bytes());
        std::iter::from_fn(|| tokenizer.next_token()).collect()
    }

    fn word(name: &str, param: Option<i32>) -> Token<'_> {
        Token::Control { name, param }
    }

    #[test]
    fn words_numbers_and_text() {
        assert_eq!(
            all("{\\rtf1\\b0 Hello\r\n world\\'e9\\par}"),
            vec![
                Token::Open,
                word("rtf", Some(1)),
                word("b", Some(0)),
                Token::Bytes(b"Hello world\xe9".to_vec()),
                word("par", None),
                Token::Close,
            ]
        );
    }

    #[test]
    fn negative_numbers_and_symbols() {
        assert_eq!(
            all("\\u-3913?\\~\\{\\*\\fi-360x"),
            vec![
                word("u", Some(-3913)),
                Token::Bytes(b"?".to_vec()),
                word("~", None),
                word("{", None),
                word("*", None),
                word("fi", Some(-360)),
                Token::Bytes(b"x".to_vec()),
            ]
        );
        // Минус без цифр — не число.
        assert_eq!(
            all("\\li-"),
            vec![word("li", None), Token::Bytes(b"-".to_vec())]
        );
    }

    #[test]
    fn binary_data_is_skipped() {
        assert_eq!(
            all("\\bin3 {}\\x"),
            vec![Token::Binary(b"{}\\"), Token::Bytes(b"x".to_vec())]
        );
    }

    #[test]
    fn broken_input_does_not_panic() {
        assert_eq!(all("\\"), vec![]);
        assert_eq!(
            all("\\'zz"),
            vec![Token::Bytes(vec![]), Token::Bytes(b"zz".to_vec())]
        );
        assert_eq!(all("\\bin999"), vec![Token::Binary(b"")]);
        assert_eq!(all("\\fs99999999999"), vec![word("fs", Some(i32::MAX))]);
    }
}
