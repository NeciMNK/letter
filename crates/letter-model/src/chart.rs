//! Диаграммы ([`Chart`]): то, что нужно, чтобы нарисовать диаграмму
//! из .docx по её кешу (последним значениям, которые сохранил Word).
//!
//! Сама часть диаграммы (`word/charts/chart1.xml` и таблица с данными)
//! хранится в пакете как есть и при сохранении не меняется; модель —
//! только для показа.

/// Диаграмма: вид, заголовок, подписи категорий и ряды значений.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chart {
    /// Вид диаграммы (по первому виду в области построения).
    pub kind: ChartKind,
    /// Как ряды стоят друг относительно друга (для столбцов, линий
    /// и областей).
    pub grouping: ChartGrouping,
    /// Заголовок; `None` — без заголовка.
    pub title: Option<String>,
    /// Подписи категорий (по оси X или секторы круга).
    pub categories: Vec<String>,
    /// Ряды значений.
    pub series: Vec<ChartSeries>,
    /// Где легенда; `None` — легенды нет.
    pub legend: Option<LegendPosition>,
    /// Каждая точка своим цветом (`c:varyColors`); у круговой —
    /// почти всегда.
    pub vary_colors: bool,
    /// Промежуток между группами столбцов, проценты ширины столбца
    /// (`c:gapWidth`, по умолчанию 150).
    pub gap_width: u16,
    /// Перекрытие столбцов одной группы, проценты (`c:overlap`, от −100
    /// до 100; у столбцов друг на друге — 100).
    pub overlap: i16,
    /// Цвета темы документа для рядов (`accent1`–`accent6`): ими
    /// раскрашиваются ряды без своего цвета.
    pub palette: Vec<(u8, u8, u8)>,
}

/// Вид диаграммы.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartKind {
    /// Гистограмма: вертикальные столбцы.
    #[default]
    Column,
    /// Линейчатая: горизонтальные полосы.
    Bar,
    /// График (линии).
    Line,
    /// С областями.
    Area,
    /// Круговая.
    Pie,
    /// Кольцевая.
    Doughnut,
    /// Точечная: значения X и Y у каждой точки.
    Scatter,
}

/// Как ряды стоят друг относительно друга.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartGrouping {
    /// Рядом (столбцы) или независимо (линии).
    #[default]
    Standard,
    /// Друг на друге.
    Stacked,
    /// Друг на друге, в сумме 100 %.
    PercentStacked,
}

/// Где легенда.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegendPosition {
    /// Снизу.
    Bottom,
    /// Сверху.
    Top,
    /// Слева.
    Left,
    /// Справа.
    Right,
}

/// Ряд значений диаграммы.
#[derive(Debug, Clone, Default)]
pub struct ChartSeries {
    /// Имя ряда (для легенды).
    pub name: String,
    /// Значения по категориям; `None` — пустая ячейка.
    pub values: Vec<Option<f64>>,
    /// Значения X точечной диаграммы (у других видов пусто).
    pub x_values: Vec<Option<f64>>,
    /// Свой цвет ряда; `None` — цвет из палитры по номеру ряда.
    pub color: Option<(u8, u8, u8)>,
    /// Свои цвета отдельных точек (номер точки и цвет).
    pub point_colors: Vec<(usize, (u8, u8, u8))>,
}

// Числа сравниваются побитно: так сравнение рефлексивно и для NaN,
// и модель может оставаться `Eq`, как всё остальное в документе.
impl PartialEq for ChartSeries {
    fn eq(&self, other: &ChartSeries) -> bool {
        let same = |a: &[Option<f64>], b: &[Option<f64>]| {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| x.map(f64::to_bits) == y.map(f64::to_bits))
        };
        self.name == other.name
            && same(&self.values, &other.values)
            && same(&self.x_values, &other.x_values)
            && self.color == other.color
            && self.point_colors == other.point_colors
    }
}

impl Eq for ChartSeries {}

impl Default for Chart {
    fn default() -> Chart {
        Chart {
            kind: ChartKind::default(),
            grouping: ChartGrouping::default(),
            title: None,
            categories: Vec::new(),
            series: Vec::new(),
            legend: None,
            vary_colors: false,
            gap_width: 150,
            overlap: 0,
            palette: Vec::new(),
        }
    }
}

impl Chart {
    /// Цвет из палитры темы для ряда (или точки, если у диаграммы
    /// каждая точка своим цветом) номер `index`.
    pub fn series_color(&self, index: usize) -> (u8, u8, u8) {
        palette_color(&self.palette, index)
    }

    /// Круговая или кольцевая (без осей).
    pub fn is_round(&self) -> bool {
        matches!(self.kind, ChartKind::Pie | ChartKind::Doughnut)
    }
}

/// Цвета темы Office по умолчанию (`accent1`–`accent6`).
pub const DEFAULT_PALETTE: [(u8, u8, u8); 6] = [
    (0x15, 0x60, 0x82),
    (0xE9, 0x71, 0x32),
    (0x19, 0x6B, 0x24),
    (0x0F, 0x9E, 0xD5),
    (0xA0, 0x2B, 0x93),
    (0x4E, 0xA7, 0x2E),
];

/// Цвет номер `index` из палитры: после шестого цвета идут они же,
/// но темнее, потом светлее (как в Word).
fn palette_color(palette: &[(u8, u8, u8)], index: usize) -> (u8, u8, u8) {
    let palette = if palette.is_empty() {
        &DEFAULT_PALETTE[..]
    } else {
        palette
    };
    let (r, g, b) = palette[index % palette.len()];
    let round = index / palette.len();
    let shade = |c: u8| -> u8 {
        match round % 3 {
            0 => c,
            1 => (f32::from(c) * 0.6).round() as u8,
            _ => (f32::from(c) + (255.0 - f32::from(c)) * 0.4).round() as u8,
        }
    };
    (shade(r), shade(g), shade(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_repeats_darker_then_lighter() {
        let chart = Chart::default();
        assert_eq!(chart.series_color(0), DEFAULT_PALETTE[0]);
        assert_eq!(chart.series_color(5), DEFAULT_PALETTE[5]);
        let (r, _, _) = chart.series_color(6);
        assert!(r < DEFAULT_PALETTE[0].0);
        let (r, _, _) = chart.series_color(12);
        assert!(r > DEFAULT_PALETTE[0].0);
    }

    #[test]
    fn series_with_nan_equals_itself() {
        let series = ChartSeries {
            values: vec![Some(f64::NAN), None, Some(1.5)],
            ..ChartSeries::default()
        };
        assert_eq!(series, series.clone());
        let other = ChartSeries {
            values: vec![Some(2.0)],
            ..ChartSeries::default()
        };
        assert_ne!(series, other);
    }
}
