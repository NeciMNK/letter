//! «Область навигации» слева от документа («Вид → Область навигации»),
//! как в Word: три вкладки —
//!
//! - «Заголовки»: дерево заголовков документа, текущий подсвечен,
//!   ветви сворачиваются; щелчок — переход к заголовку;
//! - «Страницы»: уменьшенные листы; щелчок — переход к листу;
//! - «Результаты»: поиск текста со списком найденного в окружении
//!   соседних слов; щелчок — выделить найденное.
//!
//! Как и лента, сама ничего не делает — возвращает [`NavAction`].

use std::collections::HashSet;

use eframe::egui::{self, Color32, Painter, Rect, RichText, Sense, Stroke, Ui, vec2};

use crate::i18n::{tr, tr_args};
use crate::icons::{self, Icon};
use crate::theme::Palette;

/// Вкладка области.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavTab {
    /// Заголовки.
    #[default]
    Headings,
    /// Страницы.
    Pages,
    /// Результаты поиска.
    Results,
}

/// Заголовок в дереве.
#[derive(Debug, Clone, PartialEq)]
pub struct NavHeading {
    /// Уровень, 1–9.
    pub level: u8,
    /// Текст.
    pub title: String,
}

/// Найденное: текст до, само найденное и текст после.
pub type NavResult = (String, String, String);

/// Что показать в области.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NavInfo {
    /// Заголовки документа по порядку.
    pub headings: Vec<NavHeading>,
    /// Заголовок, в разделе которого курсор (номер в `headings`).
    pub current_heading: Option<usize>,
    /// Размеры листов, пт.
    pub pages: Vec<(f32, f32)>,
    /// Лист в середине окна.
    pub current_page: usize,
    /// Найденное по строке поиска.
    pub results: Vec<NavResult>,
    /// Выделенное найденное (номер в `results`).
    pub current_result: Option<usize>,
}

/// Что выбрано в области.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavAction {
    /// Перейти к заголовку (номер в [`NavInfo::headings`]).
    Heading(usize),
    /// Перейти к листу.
    Page(usize),
    /// Выделить найденное (номер в [`NavInfo::results`]).
    Result(usize),
    /// Закрыть область.
    Close,
}

/// Состояние области.
#[derive(Debug, Clone, Default)]
pub struct NavigationPane {
    /// Область показана.
    pub open: bool,
    /// Открытая вкладка.
    pub tab: NavTab,
    /// Строка поиска.
    pub query: String,
    /// Свёрнутые заголовки (номера в списке заголовков).
    collapsed: HashSet<usize>,
}

/// Ширина уменьшенного листа на вкладке «Страницы», точки.
const THUMBNAIL_WIDTH: f32 = 130.0;

impl NavigationPane {
    /// Рисует область. `thumbnail` рисует лист (номер с нуля) в
    /// прямоугольник.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        info: &NavInfo,
        thumbnail: &mut dyn FnMut(&Painter, usize, Rect),
    ) -> Option<NavAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("nav-title")).size(16.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let name = tr("nav-close");
                if icons::icon_button(ui, Icon::Close, name, vec2(22.0, 22.0), true, false)
                    .on_hover_text(name)
                    .clicked()
                {
                    action = Some(NavAction::Close);
                }
            });
        });
        let search = egui::TextEdit::singleline(&mut self.query)
            .hint_text(tr("nav-search-hint"))
            .desired_width(f32::INFINITY);
        let response = ui.add(search);
        if response.changed() && !self.query.is_empty() {
            self.tab = NavTab::Results;
        }
        ui.horizontal(|ui| {
            for (tab, name) in [
                (NavTab::Headings, tr("nav-headings")),
                (NavTab::Pages, tr("nav-pages")),
                (NavTab::Results, tr("nav-results")),
            ] {
                ui.selectable_value(&mut self.tab, tab, name);
            }
        });
        ui.separator();
        let picked = match self.tab {
            NavTab::Headings => self.headings(ui, info),
            NavTab::Pages => pages(ui, info, thumbnail),
            NavTab::Results => self.results(ui, info),
        };
        action.or(picked)
    }

    /// Дерево заголовков.
    fn headings(&mut self, ui: &mut Ui, info: &NavInfo) -> Option<NavAction> {
        if info.headings.is_empty() {
            ui.label(RichText::new(tr("nav-no-headings")).color(Color32::GRAY));
            return None;
        }
        let mut action = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let mut hidden_below: Option<u8> = None;
                for (i, heading) in info.headings.iter().enumerate() {
                    // Внутри свёрнутой ветви — пропуск до заголовка того
                    // же или более высокого уровня.
                    match hidden_below {
                        Some(level) if heading.level > level => continue,
                        _ => hidden_below = None,
                    }
                    let has_children = info
                        .headings
                        .get(i + 1)
                        .is_some_and(|next| next.level > heading.level);
                    let collapsed = self.collapsed.contains(&i);
                    if collapsed {
                        hidden_below = Some(heading.level);
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(f32::from(heading.level.saturating_sub(1)) * 14.0);
                        if has_children {
                            let (icon, name) = if collapsed {
                                (Icon::ChevronRight, tr("nav-expand"))
                            } else {
                                (Icon::ChevronDown, tr("nav-collapse"))
                            };
                            let size = vec2(16.0, 18.0);
                            if icons::icon_button(ui, icon, name, size, true, false)
                                .on_hover_text(name)
                                .clicked()
                                && !self.collapsed.remove(&i)
                            {
                                self.collapsed.insert(i);
                            }
                        } else {
                            ui.add_space(16.0 + ui.spacing().item_spacing.x);
                        }
                        let current = info.current_heading == Some(i);
                        let text = RichText::new(&heading.title);
                        let button = egui::Button::selectable(current, text).truncate();
                        if ui.add(button).clicked() {
                            action = Some(NavAction::Heading(i));
                        }
                    });
                }
            });
        action
    }

    /// Список найденного.
    fn results(&mut self, ui: &mut Ui, info: &NavInfo) -> Option<NavAction> {
        if self.query.is_empty() {
            ui.label(RichText::new(tr("nav-search-help")).color(Color32::GRAY));
            return None;
        }
        if info.results.is_empty() {
            ui.label(tr("nav-nothing-found"));
            return None;
        }
        let count = tr_args("nav-results-count", &[("n", info.results.len().into())]);
        ui.label(RichText::new(count).strong());
        let mut action = None;
        let accent = Palette::of(ui.ctx()).heading;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                for (i, (before, found, after)) in info.results.iter().enumerate() {
                    let mut job = egui::text::LayoutJob::default();
                    let body = egui::TextStyle::Body.resolve(ui.style());
                    let normal = egui::TextFormat::simple(body.clone(), ui.visuals().text_color());
                    let marked = egui::TextFormat {
                        color: accent,
                        underline: Stroke::new(1.0, accent),
                        ..egui::TextFormat::simple(body, accent)
                    };
                    job.append(before, 0.0, normal.clone());
                    job.append(found, 0.0, marked);
                    job.append(after, 0.0, normal);
                    job.wrap.max_width = ui.available_width() - 8.0;
                    let current = info.current_result == Some(i);
                    let button = egui::Button::selectable(current, job);
                    if ui.add(button).clicked() {
                        action = Some(NavAction::Result(i));
                    }
                    ui.add_space(2.0);
                }
            });
        action
    }
}

/// Уменьшенные листы.
fn pages(
    ui: &mut Ui,
    info: &NavInfo,
    thumbnail: &mut dyn FnMut(&Painter, usize, Rect),
) -> Option<NavAction> {
    let mut action = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            // Место для рамки текущего листа.
            ui.add_space(4.0);
            for (i, &(width, height)) in info.pages.iter().enumerate() {
                let scale = THUMBNAIL_WIDTH / width.max(1.0);
                let size = vec2(THUMBNAIL_WIDTH, height * scale);
                ui.vertical_centered(|ui| {
                    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                    let label = tr_args("nav-page", &[("n", (i + 1).into())]);
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, &label)
                    });
                    if ui.is_rect_visible(rect) {
                        thumbnail(ui.painter(), i, rect);
                    }
                    if i == info.current_page || response.hovered() {
                        let color = Palette::of(ui.ctx()).heading;
                        let width = if i == info.current_page { 2.5 } else { 1.0 };
                        ui.painter().rect_stroke(
                            rect.expand(3.0),
                            2.0,
                            Stroke::new(width, color),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if response.clicked() {
                        action = Some(NavAction::Page(i));
                    }
                    ui.label(RichText::new((i + 1).to_string()).small());
                });
                ui.add_space(6.0);
            }
        });
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    struct State {
        pane: NavigationPane,
        info: NavInfo,
        out: Vec<NavAction>,
        drawn: Vec<usize>,
    }

    fn harness(info: NavInfo) -> Harness<'static, State> {
        let mut pane = NavigationPane {
            open: true,
            ..NavigationPane::default()
        };
        pane.tab = NavTab::Headings;
        Harness::builder()
            .with_size(vec2(280.0, 600.0))
            .build_ui_state(
                |ui, state: &mut State| {
                    let drawn = &mut state.drawn;
                    if let Some(action) = state.pane.ui(ui, &state.info, &mut |_, page, _| {
                        drawn.push(page);
                    }) {
                        state.out.push(action);
                    }
                },
                State {
                    pane,
                    info,
                    out: Vec::new(),
                    drawn: Vec::new(),
                },
            )
    }

    fn info() -> NavInfo {
        let heading = |level, title: &str| NavHeading {
            level,
            title: title.into(),
        };
        NavInfo {
            headings: vec![
                heading(1, "Введение"),
                heading(2, "Цели"),
                heading(1, "Итоги"),
            ],
            current_heading: Some(1),
            pages: vec![(612.0, 792.0); 2],
            current_page: 0,
            results: vec![("до ".into(), "кот".into(), " после".into())],
            current_result: None,
        }
    }

    #[test]
    fn headings_tree_navigates_and_collapses() {
        let mut h = harness(info());
        h.run();
        h.get_by_label("Итоги").click();
        h.run();
        assert_eq!(h.state().out, vec![NavAction::Heading(2)]);
        // Свернуть «Введение»: «Цели» скрываются.
        h.get_by_label("Свернуть").click();
        h.run();
        assert!(h.query_by_label("Цели").is_none());
        h.get_by_label("Развернуть").click();
        h.run();
        assert!(h.query_by_label("Цели").is_some());
    }

    #[test]
    fn pages_and_results() {
        let mut h = harness(info());
        h.get_by_label("Страницы").click();
        h.run();
        assert!(h.state().drawn.contains(&1));
        h.get_by_label("Страница 2").click();
        h.run();
        assert_eq!(h.state().out.last(), Some(&NavAction::Page(1)));

        h.state_mut().pane.query = "кот".into();
        h.get_by_label("Результаты").click();
        h.run();
        assert!(h.query_by_label("Найдено: 1").is_some());
        h.get_by_label("до кот после").click();
        h.run();
        assert_eq!(h.state().out.last(), Some(&NavAction::Result(0)));
        h.get_by_label("Закрыть область навигации").click();
        h.run();
        assert_eq!(h.state().out.last(), Some(&NavAction::Close));
    }
}
