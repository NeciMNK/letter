//! Готовые стили таблиц для галереи «Стили таблиц» на вкладке
//! «Конструктор таблиц». Стили свои (не копии стилей Word), но устроены
//! так же: оформление строки заголовка, полос, первого столбца и итоговой
//! строки, цвета — из темы документа (`w:themeColor`), поэтому таблица
//! выглядит одинаково и у нас, и в Word.

use letter_model::{
    Border, Borders, Color, Document, LineSpacing, Shading, Style, StyleKind, TableArea,
    TableAreaStyle, ThemeColor, Twips,
};

/// Вид готового стиля таблицы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TablePreset {
    /// Без вертикальных линий: линии сверху и снизу таблицы и под
    /// заголовком, полосы через строку.
    Plain,
    /// Сетка: все линии, заголовок залит цветом.
    Grid,
    /// Список: рамка вокруг таблицы, заголовок залит цветом, полосы.
    List,
}

impl TablePreset {
    /// Все виды по порядку галереи.
    pub const ALL: [TablePreset; 3] = [TablePreset::Plain, TablePreset::Grid, TablePreset::List];

    fn name(self) -> &'static str {
        match self {
            TablePreset::Plain => "Plain",
            TablePreset::Grid => "Grid",
            TablePreset::List => "List",
        }
    }
}

/// Цвета готовых стилей: «Текст 1» и шесть акцентов темы — имя в
/// `w:themeColor` и в схеме цветов темы.
pub const PRESET_COLORS: [(&str, &str); 7] = [
    ("text1", "dk1"),
    ("accent1", "accent1"),
    ("accent2", "accent2"),
    ("accent3", "accent3"),
    ("accent4", "accent4"),
    ("accent5", "accent5"),
    ("accent6", "accent6"),
];

/// Осветление полос: 20 % цвета.
const BAND_TINT: u8 = 0x33;
/// Осветление линий сетки: 60 % цвета.
const LINE_TINT: u8 = 0x99;

/// Готовый стиль `preset` цвета номер `color` из [`PRESET_COLORS`]
/// (номер больше шести — шестой) с цветами темы документа `doc`.
pub fn table_preset(doc: &Document, preset: TablePreset, color: usize) -> Style {
    let (theme, scheme) = PRESET_COLORS[color.min(PRESET_COLORS.len() - 1)];
    let [r, g, b] = doc.theme.colors.color(scheme);
    let tinted = |tint: Option<u8>| {
        let keep = f32::from(tint.unwrap_or(255)) / 255.0;
        let mix = |c: u8| (f32::from(c) + (255.0 - f32::from(c)) * (1.0 - keep)).round() as u8;
        (
            Color::Rgb(mix(r), mix(g), mix(b)),
            ThemeColor {
                name: theme.to_string(),
                shade: None,
                tint,
            },
        )
    };
    let line = |tint: Option<u8>, eighths: u32| {
        let (color, theme) = tinted(tint);
        Border {
            color: Some(color),
            theme_color: Some(theme),
            ..Border::single(eighths)
        }
    };
    let fill = |tint: Option<u8>| {
        let (color, theme) = tinted(tint);
        Shading {
            theme_fill: Some(theme),
            ..Shading::fill(color)
        }
    };

    let label = if color == 0 {
        "Text 1".to_string()
    } else {
        format!("Accent {color}")
    };
    let mut style = Style::new(
        format!("Letter{}{}", preset.name(), label.replace(' ', "")),
        StyleKind::Table,
    );
    style.name = Some(format!("Letter {} {label}", preset.name()));
    style.custom = true;
    style.paragraph.spacing.after = Some(Twips(0));
    style.paragraph.spacing.line = Some(LineSpacing::SINGLE);
    style.table.row_band_size = Some(1);
    style.table.column_band_size = Some(1);

    let mut header = TableAreaStyle::new(TableArea::FirstRow);
    header.run.bold = Some(true);
    let mut band = TableAreaStyle::new(TableArea::Band1Horizontal);
    band.cell.shading = Some(fill(Some(BAND_TINT)));
    let mut first_column = TableAreaStyle::new(TableArea::FirstColumn);
    first_column.run.bold = Some(true);
    let mut last_row = TableAreaStyle::new(TableArea::LastRow);
    last_row.run.bold = Some(true);
    match preset {
        TablePreset::Plain => {
            style.table.borders = Some(Borders {
                top: Some(line(None, 8)),
                bottom: Some(line(None, 8)),
                ..Borders::default()
            });
            header.cell.borders = Some(Borders {
                bottom: Some(line(None, 8)),
                ..Borders::default()
            });
            last_row.cell.borders = Some(Borders {
                top: Some(line(None, 4)),
                ..Borders::default()
            });
        }
        TablePreset::Grid | TablePreset::List => {
            let thin = if preset == TablePreset::Grid {
                Borders::all(line(Some(LINE_TINT), 4))
            } else {
                Borders {
                    top: Some(line(None, 4)),
                    left: Some(line(None, 4)),
                    bottom: Some(line(None, 4)),
                    right: Some(line(None, 4)),
                    ..Borders::default()
                }
            };
            style.table.borders = Some(thin);
            header.cell.shading = Some(fill(None));
            header.run.color = Some(Color::Rgb(0xFF, 0xFF, 0xFF));
            header.run.theme_color = Some(ThemeColor {
                name: "background1".into(),
                shade: None,
                tint: None,
            });
            last_row.cell.borders = Some(Borders {
                top: Some(Border {
                    style: "double".into(),
                    ..line(None, 4)
                }),
                ..Borders::default()
            });
        }
    }
    style.table_areas = vec![band, first_column, header, last_row];
    style
}

/// Все готовые стили: по видам, внутри вида — по цветам.
pub fn table_presets(doc: &Document) -> Vec<Style> {
    TablePreset::ALL
        .iter()
        .flat_map(|&preset| (0..PRESET_COLORS.len()).map(move |c| table_preset(doc, preset, c)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_take_colors_from_the_theme() {
        let doc = Document::new();
        let all = table_presets(&doc);
        assert_eq!(all.len(), 21);
        let ids: std::collections::HashSet<_> = all.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids.len(), 21);
        let grid = table_preset(&doc, TablePreset::Grid, 1);
        assert_eq!(grid.id, "LetterGridAccent1");
        let header = grid
            .table_areas
            .iter()
            .find(|a| a.area == TableArea::FirstRow)
            .unwrap();
        let shading = header.cell.shading.as_ref().unwrap();
        let [r, g, b] = doc.theme.colors.color("accent1");
        assert_eq!(shading.fill, Some(Color::Rgb(r, g, b)));
        assert_eq!(shading.theme_fill.as_ref().unwrap().name, "accent1");
        // Полоса — светлее: 20 % цвета.
        let band = grid
            .table_areas
            .iter()
            .find(|a| a.area == TableArea::Band1Horizontal)
            .unwrap();
        let Some(Color::Rgb(br, ..)) = band.cell.shading.as_ref().unwrap().fill else {
            panic!("нет заливки полосы");
        };
        assert!(br > r);
    }
}
