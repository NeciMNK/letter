//! Значки интерфейса из открытого набора Lucide (лицензия ISC, не значки
//! Microsoft). Набор подключён шрифтом `assets/lucide/lucide.ttf`: каждый
//! значок — один символ. Как добавить значок — `assets/lucide/README.md`.

use eframe::egui::{
    Align2, Color32, Context, CornerRadius, FontData, FontFamily, FontId, Painter, Rect, Response,
    Sense, Ui, Vec2, WidgetInfo,
    epaint::text::{FontInsert, FontPriority, InsertFontFamily},
    pos2, vec2,
};
use letter_model::Alignment;

/// Значок.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Сохранить: дискета.
    Save,
    /// Отменить: стрелка назад.
    Undo,
    /// Повторить: стрелка вперёд.
    Redo,
    /// Вырезать: ножницы.
    Cut,
    /// Копировать: два листа.
    Copy,
    /// Вставить: планшет с листом.
    Paste,
    /// Выравнивание абзаца: строки.
    Align(Alignment),
    /// Увеличить размер шрифта.
    Grow,
    /// Уменьшить размер шрифта.
    Shrink,
}

/// Шрифт со значками.
const FONT: &[u8] = include_bytes!("../assets/lucide/lucide.ttf");

/// Имя семейства шрифта со значками в egui.
const FAMILY: &str = "lucide";

impl Icon {
    /// Символ значка в шрифте.
    pub fn glyph(self) -> char {
        self.entry().1
    }

    /// Имя значка в наборе Lucide и его символ.
    fn entry(self) -> (&'static str, char) {
        match self {
            Icon::Save => ("save", '\u{E14D}'),
            Icon::Undo => ("undo-2", '\u{E2A1}'),
            Icon::Redo => ("redo-2", '\u{E2A0}'),
            Icon::Cut => ("scissors", '\u{E14E}'),
            Icon::Copy => ("copy", '\u{E09E}'),
            Icon::Paste => ("clipboard-paste", '\u{E3E8}'),
            Icon::Align(Alignment::Left) => ("text-align-start", '\u{E185}'),
            Icon::Align(Alignment::Center) => ("text-align-center", '\u{E182}'),
            Icon::Align(Alignment::Right) => ("text-align-end", '\u{E183}'),
            Icon::Align(Alignment::Justify | Alignment::Distribute) => {
                ("text-align-justify", '\u{E184}')
            }
            Icon::Grow => ("a-arrow-up", '\u{E586}'),
            Icon::Shrink => ("a-arrow-down", '\u{E585}'),
        }
    }
}

/// Кнопка-значок размера `size`. `name` — название кнопки для подсказки
/// и для программ чтения с экрана (и тестов).
pub fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    name: &str,
    size: Vec2,
    enabled: bool,
    selected: bool,
) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response
        .widget_info(|| WidgetInfo::selected(eframe::egui::Role::Button, enabled, selected, name));
    let visuals = ui.style().interact_selectable(&response, selected);
    if enabled && (selected || response.hovered()) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(2), visuals.weak_bg_fill);
    }
    let color = if enabled {
        visuals.fg_stroke.color
    } else {
        Color32::from_gray(170)
    };
    let side = rect.height().min(rect.width()).min(32.0) * 0.7;
    let area = Rect::from_center_size(rect.center(), Vec2::splat(side));
    paint(ui.painter(), area, icon, color);
    response
}

/// Кнопка «значок и надпись» в одну строку.
pub fn icon_text_button(ui: &mut Ui, icon: Icon, text: &str, enabled: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        eframe::egui::TextStyle::Body.resolve(ui.style()),
        Color32::PLACEHOLDER,
    );
    let height = 20.0_f32.max(galley.size().y + 4.0);
    let size = vec2(height + 4.0 + galley.size().x + 4.0, height);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::labeled(eframe::egui::Role::Button, enabled, text));
    let visuals = ui.style().interact(&response);
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(2), visuals.weak_bg_fill);
    }
    let color = if enabled {
        visuals.fg_stroke.color
    } else {
        Color32::from_gray(170)
    };
    let icon_rect = Rect::from_min_size(rect.min, Vec2::splat(height));
    let side = height * 0.7;
    paint(
        ui.painter(),
        Rect::from_center_size(icon_rect.center(), Vec2::splat(side)),
        icon,
        color,
    );
    let text_pos = pos2(
        icon_rect.right() + 4.0,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(text_pos, galley, color);
    response
}

/// Рисует значок в квадрате `r`.
pub fn paint(painter: &Painter, r: Rect, icon: Icon, color: Color32) {
    let ctx = painter.ctx();
    install(ctx);
    if !ready(ctx) {
        // Шрифт подключится к следующему кадру.
        ctx.request_repaint();
        return;
    }
    painter.text(
        r.center(),
        Align2::CENTER_CENTER,
        icon.glyph(),
        FontId::new(r.height(), family()),
        color,
    );
}

/// Семейство шрифта со значками.
fn family() -> FontFamily {
    FontFamily::Name(FAMILY.into())
}

/// Подключает шрифт со значками; он появится к началу следующего кадра
/// (или текущего, если в нём ещё не было текста). Можно вызывать и до
/// первого кадра; повторный вызов ничего не стоит.
pub fn install(ctx: &Context) {
    ctx.add_font(FontInsert::new(
        FAMILY,
        FontData::from_static(FONT),
        vec![InsertFontFamily {
            family: family(),
            priority: FontPriority::Highest,
        }],
    ));
}

/// Готов ли шрифт со значками. Только во время кадра.
fn ready(ctx: &Context) -> bool {
    ctx.fonts(|f| f.definitions().families.contains_key(&family()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Icon; 13] = [
        Icon::Save,
        Icon::Undo,
        Icon::Redo,
        Icon::Cut,
        Icon::Copy,
        Icon::Paste,
        Icon::Align(Alignment::Left),
        Icon::Align(Alignment::Center),
        Icon::Align(Alignment::Right),
        Icon::Align(Alignment::Justify),
        Icon::Align(Alignment::Distribute),
        Icon::Grow,
        Icon::Shrink,
    ];

    #[test]
    fn codes_match_the_font() {
        let codes = include_str!("../assets/lucide/codes.txt");
        for icon in ALL {
            let code = format!("{:X}", icon.glyph() as u32);
            let line = codes
                .lines()
                .find(|l| l.split(' ').next() == Some(icon.entry().0))
                .unwrap_or_else(|| panic!("нет значка {}", icon.entry().0));
            assert_eq!(line, format!("{} {code}", icon.entry().0));
        }
    }

    #[test]
    fn font_has_the_glyphs() {
        let mut h = egui_kittest::Harness::new_ui(|ui| {
            install(ui.ctx());
        });
        h.run();
        assert!(ready(&h.ctx));
        let id = FontId::new(16.0, family());
        for icon in ALL {
            assert!(
                h.ctx.fonts_mut(|f| f.has_glyph(&id, icon.glyph())),
                "{icon:?}"
            );
        }
    }
}
