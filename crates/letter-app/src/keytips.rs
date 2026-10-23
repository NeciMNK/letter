//! Подсказки клавиш, как в Word: нажать и отпустить Alt — на вкладках
//! появятся буквы; буква вкладки открывает её и показывает буквы команд;
//! буквы команды выполняют её. Esc — на шаг назад, Alt или щелчок —
//! выход. Alt+буква сразу выбирает вкладку.
//!
//! Буквы берутся из файлов перевода (`keytip-…`): в русском интерфейсе
//! они русские, в английском — как в английском Word. Клавиши
//! сравниваются по месту на клавиатуре, поэтому раскладка не важна.

use eframe::egui::{
    self, Color32, Context, CornerRadius, Event, FontId, Id, Key, LayerId, Order, Rect, Response,
    Stroke, Ui, vec2,
};

use crate::i18n::{Language, language, tr};
use crate::ribbon::{RibbonCommand, Tab};

/// Что видно: буквы вкладок или буквы команд открытой вкладки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Вкладки, «Файл» и панель быстрого доступа.
    Tabs,
    /// Команды вкладки.
    Commands,
}

/// Что делает подсказка.
#[derive(Debug, Clone, PartialEq)]
pub enum TipAction {
    /// Выполнить команду.
    Command(RibbonCommand),
    /// Открыть вкладку и показать её буквы.
    Tab(Tab),
    /// Открыть список или меню (по id всплывающего окна egui).
    Popup(Id),
    /// Поставить фокус в поле ввода.
    Focus(Id),
}

/// Подсказка на экране: где, какие буквы, что делает.
#[derive(Debug, Clone, PartialEq)]
pub struct Tip {
    /// Место кнопки.
    pub rect: Rect,
    /// Буквы (как в файле перевода).
    pub keys: String,
    /// Действие.
    pub action: TipAction,
    /// Кнопка доступна.
    pub enabled: bool,
}

/// Состояние подсказок.
#[derive(Debug, Clone, Default)]
pub struct KeyTips {
    /// Видимый уровень; `None` — подсказок нет.
    pub level: Option<Level>,
    /// Уже набранные буквы многобуквенной подсказки.
    pub typed: String,
    /// Alt нажат (ждём, отпустят ли его без других клавиш).
    alt_down: bool,
    /// Пока Alt был нажат, нажимали что-то ещё.
    alt_used: bool,
}

/// Где в памяти egui лежат подсказки кадра.
fn tips_id() -> Id {
    Id::NULL.with("letter-keytips")
}

/// Уровень, для которого собираются подсказки в этом кадре.
fn level_id() -> Id {
    Id::NULL.with("letter-keytips-level")
}

/// Подсказки вкладок: «Файл», вкладки, панель быстрого доступа. Они
/// известны заранее, поэтому Alt+буква работает сразу.
pub fn tab_tips() -> Vec<(&'static str, TipAction)> {
    let mut tips = vec![
        ("keytip-file", TipAction::Command(RibbonCommand::Backstage)),
        ("keytip-save", TipAction::Command(RibbonCommand::Save)),
        ("keytip-undo", TipAction::Command(RibbonCommand::Undo)),
        ("keytip-redo", TipAction::Command(RibbonCommand::Redo)),
    ];
    for tab in Tab::ALL.into_iter().chain([
        Tab::TableDesign,
        Tab::TableLayout,
        Tab::PictureFormat,
        Tab::HeaderFooter,
    ]) {
        tips.push((tab.keytip(), TipAction::Tab(tab)));
    }
    tips
}

/// Начинает кадр ленты: старые подсказки убираются.
pub fn begin(ctx: &Context, level: Option<Level>) {
    ctx.data_mut(|d| {
        d.insert_temp(level_id(), level);
        d.insert_temp(tips_id(), Vec::<Tip>::new());
    });
}

/// Отмечает кнопку `response` подсказкой `id` (имя из файла перевода),
/// если сейчас видны подсказки уровня `level`.
pub fn tip(ui: &Ui, response: &Response, level: Level, id: &str, action: TipAction) {
    let current: Option<Level> = ui.ctx().data(|d| d.get_temp(level_id())).flatten();
    if current != Some(level) {
        return;
    }
    let tip = Tip {
        rect: response.rect,
        keys: tr(id).to_owned(),
        action,
        enabled: response.enabled(),
    };
    ui.ctx()
        .data_mut(|d| d.get_temp_mut_or_default::<Vec<Tip>>(tips_id()).push(tip));
}

/// Подсказки, собранные в последнем кадре.
pub fn collected(ctx: &Context) -> Vec<Tip> {
    ctx.data(|d| d.get_temp(tips_id())).unwrap_or_default()
}

/// Буква клавиши по её месту: латинская в английском интерфейсе,
/// русская (ЙЦУКЕН) — в русском. Цифры — как есть.
pub fn key_char(key: Key) -> Option<char> {
    let name = key.name();
    let mut chars = name.chars();
    let c = chars.next()?;
    if chars.next().is_some() || !c.is_ascii_alphanumeric() {
        return None;
    }
    if c.is_ascii_digit() || language() == Language::English {
        return Some(c);
    }
    const LATIN: &str = "QWERTYUIOPASDFGHJKLZXCVBNM";
    const CYRILLIC: [char; 26] = [
        'Й', 'Ц', 'У', 'К', 'Е', 'Н', 'Г', 'Ш', 'Щ', 'З', 'Ф', 'Ы', 'В', 'А', 'П', 'Р', 'О', 'Л',
        'Д', 'Я', 'Ч', 'С', 'М', 'И', 'Т', 'Ь',
    ];
    LATIN.find(c).map(|i| CYRILLIC[i])
}

/// Что сделать по итогам нажатий.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Выполнить команду.
    Command(RibbonCommand),
    /// Открыть вкладку.
    Tab(Tab),
}

impl KeyTips {
    /// Подсказки видны.
    pub fn active(&self) -> bool {
        self.level.is_some()
    }

    fn close(&mut self) {
        self.level = None;
        self.typed.clear();
    }

    /// Разбирает клавиши кадра. Пока подсказки видны, все нажатия
    /// достаются им. `tips` — подсказки команд из прошлого кадра.
    pub fn handle(&mut self, ctx: &Context, tips: &[Tip]) -> Vec<Outcome> {
        let mut out = Vec::new();
        // Alt нажат и отпущен, а между ними ничего не нажимали. Клавишу
        // Alt egui присылает событием; если его нет (так бывает не везде),
        // смотрим на смену модификаторов.
        let events = ctx.input(|i| i.events.clone());
        let alt_keys = events.iter().any(
            |e| matches!(e, Event::Key { key, .. } if matches!(key, Key::AltLeft | Key::AltRight)),
        );
        let mut toggles = 0;
        for event in &events {
            let alt = match event {
                Event::Key {
                    key: Key::AltLeft | Key::AltRight,
                    pressed,
                    ..
                } => Some(*pressed),
                Event::ModifiersChanged(m) if !alt_keys => Some(m.alt),
                Event::Key { pressed: true, .. }
                | Event::PointerButton { pressed: true, .. }
                | Event::WindowFocused(false) => {
                    // Alt+Tab, Alt+F4, Alt+щелчок — не подсказки.
                    self.alt_used = true;
                    None
                }
                _ => None,
            };
            match alt {
                Some(true) if !self.alt_down => {
                    self.alt_down = true;
                    self.alt_used = false;
                }
                Some(false) if self.alt_down => {
                    self.alt_down = false;
                    if !self.alt_used {
                        toggles += 1;
                    }
                }
                _ => {}
            }
        }
        if toggles % 2 == 1 {
            if self.active() {
                self.close();
            } else {
                self.level = Some(Level::Tabs);
                self.typed.clear();
            }
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Key { .. })));
            return out;
        }
        if ctx.input(|i| i.pointer.any_pressed()) {
            self.close();
            return out;
        }
        // Alt+буква без Ctrl — сразу подсказки вкладок.
        let alt_letter = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.alt && !modifiers.command => {
                    key_char(physical_key.unwrap_or(*key)).filter(|c| c.is_alphabetic())
                }
                _ => None,
            })
        });
        if !self.active() {
            let Some(c) = alt_letter else {
                return out;
            };
            self.level = Some(Level::Tabs);
            self.typed.clear();
            self.alt_used = true;
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Key { .. })));
            self.press(c, tips, &mut out);
            return out;
        }
        let events = ctx.input_mut(|i| {
            let events: Vec<Event> = i
                .events
                .iter()
                .filter(|e| matches!(e, Event::Key { .. } | Event::Text(_)))
                .cloned()
                .collect();
            i.events
                .retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_)));
            events
        });
        for event in events {
            let Event::Key {
                key,
                physical_key,
                pressed: true,
                ..
            } = event
            else {
                continue;
            };
            if key == Key::Escape {
                if self.level == Some(Level::Commands) {
                    self.level = Some(Level::Tabs);
                    self.typed.clear();
                } else {
                    self.close();
                }
                continue;
            }
            if let Some(c) = key_char(physical_key.unwrap_or(key)) {
                self.press(c, tips, &mut out);
            }
            if !self.active() {
                break;
            }
        }
        out
    }

    /// Нажата буква `c`.
    fn press(&mut self, c: char, tips: &[Tip], out: &mut Vec<Outcome>) {
        let typed = format!("{}{c}", self.typed);
        // На уровне вкладок подсказки известны заранее.
        let choices: Vec<(String, TipAction, bool)> = match self.level {
            Some(Level::Tabs) => tab_tips()
                .into_iter()
                .map(|(id, action)| (tr(id).to_owned(), action, true))
                .collect(),
            Some(Level::Commands) => tips
                .iter()
                .map(|t| (t.keys.clone(), t.action.clone(), t.enabled))
                .collect(),
            None => return,
        };
        if let Some((_, action, enabled)) = choices.iter().find(|(keys, ..)| *keys == typed) {
            self.typed.clear();
            if !enabled {
                return;
            }
            match action.clone() {
                TipAction::Tab(tab) => {
                    self.level = Some(Level::Commands);
                    out.push(Outcome::Tab(tab));
                }
                TipAction::Command(command) => {
                    self.close();
                    out.push(Outcome::Command(command));
                }
                TipAction::Popup(id) => {
                    self.close();
                    out.push(Outcome::Command(RibbonCommand::OpenPopup(id)));
                }
                TipAction::Focus(id) => {
                    self.close();
                    out.push(Outcome::Command(RibbonCommand::Focus(id)));
                }
            }
        } else if choices.iter().any(|(keys, ..)| keys.starts_with(&typed)) {
            self.typed = typed;
        } else {
            // Такой подсказки нет: начинаем набор заново, как Word.
            self.typed.clear();
        }
    }
}

/// Рисует буквы поверх ленты: под кнопкой по центру. Подсказки, которые
/// не подходят к уже набранным буквам, не видны.
pub fn paint(ctx: &Context, tips: &[Tip], typed: &str) {
    let painter = ctx.layer_painter(LayerId::new(
        Order::Foreground,
        Id::NULL.with("keytips-layer"),
    ));
    let dark = ctx.theme() == egui::Theme::Dark;
    let (fill, text) = if dark {
        (Color32::from_gray(60), Color32::WHITE)
    } else {
        (Color32::from_gray(70), Color32::WHITE)
    };
    for tip in tips.iter().filter(|t| t.keys.starts_with(typed)) {
        let galley = painter.layout_no_wrap(tip.keys.clone(), FontId::proportional(11.0), text);
        let size = galley.size() + vec2(8.0, 2.0);
        let center = tip.rect.center_bottom() - vec2(0.0, 4.0);
        let rect = Rect::from_center_size(center, size);
        let fill = if tip.enabled {
            fill
        } else {
            fill.gamma_multiply(0.5)
        };
        painter.rect(
            rect,
            CornerRadius::same(2),
            fill,
            Stroke::NONE,
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::set_language;

    #[test]
    fn letters_by_key_place() {
        assert_eq!(key_char(Key::A), Some('Ф'));
        assert_eq!(key_char(Key::Z), Some('Я'));
        assert_eq!(key_char(Key::Num1), Some('1'));
        assert_eq!(key_char(Key::Enter), None);
        set_language(Language::English);
        assert_eq!(key_char(Key::A), Some('A'));
        set_language(Language::Russian);
    }

    /// Ни одна подсказка уровня не начало другой: иначе до длинной не
    /// добраться.
    fn no_prefixes(keys: &[String]) {
        for a in keys {
            for b in keys {
                assert!(a == b || !b.starts_with(a.as_str()), "{a} и {b}");
            }
        }
    }

    #[test]
    fn tab_tips_are_unique() {
        for lang in Language::ALL {
            set_language(lang);
            let keys: Vec<String> = tab_tips().iter().map(|(id, _)| tr(id).to_owned()).collect();
            no_prefixes(&keys);
            let mut sorted = keys.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), keys.len(), "{keys:?}");
            // Каждую букву можно набрать.
            for key in &keys {
                for c in key.chars() {
                    let typed = Key::ALL.iter().any(|k| key_char(*k) == Some(c));
                    assert!(typed, "{c}");
                }
            }
        }
        set_language(Language::Russian);
    }

    #[test]
    fn command_tips_are_unique() {
        for lang in Language::ALL {
            set_language(lang);
            for ids in [
                &crate::home::KEYTIPS[..],
                &crate::ribbon::VIEW_KEYTIPS[..],
                &crate::insert::INSERT_KEYTIPS[..],
                &crate::insert::TABLE_KEYTIPS[..],
                &crate::insert::DESIGN_KEYTIPS[..],
                &crate::insert::PICTURE_KEYTIPS[..],
                &crate::layout::KEYTIPS[..],
                &crate::headers::KEYTIPS[..],
                &crate::references::KEYTIPS[..],
                &crate::comments::KEYTIPS[..],
                &crate::outlining::KEYTIPS[..],
                &crate::design::KEYTIPS[..],
                &crate::review::KEYTIPS[..],
            ] {
                let keys: Vec<String> = ids.iter().map(|id| tr(id).to_owned()).collect();
                no_prefixes(&keys);
                for key in &keys {
                    for c in key.chars() {
                        assert!(Key::ALL.iter().any(|k| key_char(*k) == Some(c)), "{c}");
                    }
                }
            }
        }
        set_language(Language::Russian);
    }
}
