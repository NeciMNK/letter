//! Сочетания клавиш, как в Word. Одна таблица на всю программу: по ней
//! работают клавиши, и по ней же можно показать их список.

use eframe::egui::{Key, Modifiers};
use letter_editor::{Command, ParagraphFormat, RunFormat};
use letter_model::{Alignment, LineSpacing, NoteKind};

use crate::ribbon::RibbonCommand;

/// Сочетание клавиш и что оно делает.
#[derive(Debug, Clone, PartialEq)]
pub struct Shortcut {
    /// Ctrl, Shift, Alt.
    pub modifiers: Modifiers,
    /// Клавиша. Для сочетаний с Shift — клавиша без Shift («=» в
    /// Ctrl+Shift+=), её ищем и по месту на клавиатуре.
    pub key: Key,
    /// Команда.
    pub command: RibbonCommand,
    /// Работает и без открытого документа, и когда фокус в поле ввода
    /// на ленте (Ctrl+N, Ctrl+O, Ctrl+S…).
    pub global: bool,
}

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
const CTRL_ALT: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT);

/// Все сочетания.
pub fn table() -> Vec<Shortcut> {
    use RibbonCommand as C;
    let global = |modifiers, key, command| Shortcut {
        modifiers,
        key,
        command,
        global: true,
    };
    let local = |modifiers, key, command| Shortcut {
        modifiers,
        key,
        command,
        global: false,
    };
    let run = |modifiers, key, format| local(modifiers, key, C::Format(format));
    let para = |modifiers, key, format| local(modifiers, key, C::Paragraph(format));
    let align = |key, alignment| local(CTRL, key, C::Align(alignment));
    let spacing = |key, line| para(CTRL, key, ParagraphFormat::LineSpacing(line));
    vec![
        // Файлы.
        global(CTRL, Key::N, C::New),
        global(CTRL, Key::O, C::Open),
        global(CTRL, Key::S, C::Save),
        global(Modifiers::NONE, Key::F12, C::SaveAs),
        global(CTRL, Key::P, C::PrintPreview),
        global(CTRL, Key::F2, C::PrintPreview),
        global(CTRL, Key::W, C::CloseDocument),
        global(CTRL, Key::F4, C::CloseDocument),
        global(Modifiers::ALT, Key::F4, C::Exit),
        // Правка.
        local(CTRL, Key::Z, C::Undo),
        local(Modifiers::ALT, Key::Backspace, C::Undo),
        local(CTRL, Key::Y, C::Redo),
        local(CTRL_SHIFT, Key::Z, C::Redo),
        local(CTRL, Key::A, C::SelectAll),
        local(CTRL, Key::F, C::Find),
        local(CTRL, Key::H, C::Replace),
        local(CTRL, Key::K, C::LinkDialog),
        local(CTRL, Key::Backspace, C::DeleteWord { forward: false }),
        local(CTRL, Key::Delete, C::DeleteWord { forward: true }),
        local(CTRL_SHIFT, Key::Space, C::Insert("\u{a0}")),
        local(CTRL_SHIFT, Key::Minus, C::Insert("\u{2011}")),
        local(CTRL, Key::Minus, C::Insert("\u{ad}")),
        local(CTRL_SHIFT, Key::Num8, C::ToggleMarks),
        local(Modifiers::NONE, Key::F9, C::UpdateFields),
        local(
            Modifiers::NONE,
            Key::F7,
            C::Spelling(crate::spelling::SpellAction::Next),
        ),
        local(
            CTRL_ALT,
            Key::F,
            C::Edit(Command::InsertNote(NoteKind::Footnote)),
        ),
        local(
            CTRL_ALT,
            Key::D,
            C::Edit(Command::InsertNote(NoteKind::Endnote)),
        ),
        // Шрифт.
        local(CTRL, Key::D, C::FontDialog),
        run(CTRL, Key::B, RunFormat::Bold),
        run(CTRL, Key::I, RunFormat::Italic),
        run(CTRL, Key::U, RunFormat::Underline),
        run(CTRL_SHIFT, Key::D, RunFormat::DoubleUnderline),
        run(CTRL_SHIFT, Key::W, RunFormat::WordUnderline),
        run(CTRL_SHIFT, Key::A, RunFormat::Caps),
        run(CTRL_SHIFT, Key::K, RunFormat::SmallCaps),
        run(CTRL_SHIFT, Key::H, RunFormat::Hidden),
        run(CTRL, Key::Equals, RunFormat::Subscript),
        run(CTRL_SHIFT, Key::Equals, RunFormat::Superscript),
        run(CTRL, Key::CloseBracket, RunFormat::Grow),
        run(CTRL, Key::OpenBracket, RunFormat::Shrink),
        run(CTRL_SHIFT, Key::Period, RunFormat::GrowStep),
        run(CTRL_SHIFT, Key::Comma, RunFormat::ShrinkStep),
        run(CTRL, Key::Space, RunFormat::Clear),
        // Абзац.
        align(Key::L, Alignment::Left),
        align(Key::E, Alignment::Center),
        align(Key::R, Alignment::Right),
        align(Key::J, Alignment::Justify),
        para(CTRL, Key::M, ParagraphFormat::IndentMore),
        para(CTRL_SHIFT, Key::M, ParagraphFormat::IndentLess),
        para(CTRL, Key::T, ParagraphFormat::HangingMore),
        para(CTRL_SHIFT, Key::T, ParagraphFormat::HangingLess),
        spacing(Key::Num1, LineSpacing::SINGLE),
        spacing(Key::Num2, LineSpacing::DOUBLE),
        spacing(Key::Num5, LineSpacing::ONE_AND_HALF),
        para(CTRL, Key::Num0, ParagraphFormat::ToggleSpaceBefore),
        para(CTRL, Key::Q, ParagraphFormat::Reset),
        // Стили.
        local(CTRL_SHIFT, Key::N, C::BuiltInStyle("normal")),
        local(CTRL_ALT, Key::Num1, C::BuiltInStyle("heading 1")),
        local(CTRL_ALT, Key::Num2, C::BuiltInStyle("heading 2")),
        local(CTRL_ALT, Key::Num3, C::BuiltInStyle("heading 3")),
        local(CTRL_ALT, Key::M, C::NewComment),
        local(CTRL_SHIFT, Key::E, C::ToggleTrackRevisions),
        local(CTRL_SHIFT, Key::G, C::WordCount),
    ]
}

/// Подходит ли нажатие к сочетанию. С Shift клавиша даёт другой знак
/// («=» → «+», «.» → «>»), поэтому тогда смотрим и на место клавиши.
/// При русской раскладке буквы приходят по месту клавиши (Ctrl+И — это
/// Ctrl+B), как в Word.
pub fn matches(shortcut: &Shortcut, key: Key, physical: Option<Key>, modifiers: Modifiers) -> bool {
    let wanted = shortcut.modifiers;
    let same_modifiers = modifiers.command == wanted.command
        && modifiers.shift == wanted.shift
        && modifiers.alt == wanted.alt;
    same_modifiers && (key == shortcut.key || (wanted.shift && physical == Some(shortcut.key)))
}

/// Команда для нажатия; `global_only` — только общие сочетания.
pub fn find(
    table: &[Shortcut],
    key: Key,
    physical: Option<Key>,
    modifiers: Modifiers,
    global_only: bool,
) -> Option<RibbonCommand> {
    table
        .iter()
        .filter(|s| s.global || !global_only)
        .find(|s| matches(s, key, physical, modifiers))
        .map(|s| s.command.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key, modifiers: Modifiers) -> Option<RibbonCommand> {
        find(&table(), key, Some(key), modifiers, false)
    }

    #[test]
    fn no_duplicates() {
        let table = table();
        for (i, a) in table.iter().enumerate() {
            for b in &table[i + 1..] {
                assert!(
                    !(a.key == b.key && a.modifiers == b.modifiers),
                    "{a:?} и {b:?}"
                );
            }
        }
    }

    #[test]
    fn word_keys() {
        assert_eq!(
            press(Key::B, CTRL),
            Some(RibbonCommand::Format(RunFormat::Bold))
        );
        // Ctrl+Shift+B ничего не делает.
        assert_eq!(press(Key::B, CTRL_SHIFT), None);
        assert_eq!(press(Key::B, Modifiers::NONE), None);
        // Ctrl+Shift+= приходит как «+», а место клавиши — «=».
        assert_eq!(
            find(&table(), Key::Plus, Some(Key::Equals), CTRL_SHIFT, false),
            Some(RibbonCommand::Format(RunFormat::Superscript))
        );
        assert_eq!(
            press(Key::Equals, CTRL),
            Some(RibbonCommand::Format(RunFormat::Subscript))
        );
        // Неразрывный дефис и мягкий перенос.
        assert_eq!(
            find(&table(), Key::Minus, Some(Key::Minus), CTRL_SHIFT, false),
            Some(RibbonCommand::Insert("\u{2011}"))
        );
        assert_eq!(
            press(Key::Minus, CTRL),
            Some(RibbonCommand::Insert("\u{ad}"))
        );
        // Без Shift место клавиши не смотрим: при другой латинской
        // раскладке буквы стоят иначе.
        assert_eq!(
            find(&table(), Key::Q, Some(Key::A), CTRL, false),
            Some(RibbonCommand::Paragraph(ParagraphFormat::Reset))
        );
        assert_eq!(
            press(Key::Num1, CTRL_ALT),
            Some(RibbonCommand::BuiltInStyle("heading 1"))
        );
        assert_eq!(press(Key::S, CTRL), Some(RibbonCommand::Save));
        assert_eq!(
            find(&table(), Key::B, Some(Key::B), CTRL, true),
            None,
            "Ctrl+B — не общее сочетание"
        );
        assert_eq!(
            find(&table(), Key::S, Some(Key::S), CTRL, true),
            Some(RibbonCommand::Save)
        );
    }
}
