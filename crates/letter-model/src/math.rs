//! Формулы ([`Math`]) — то, что в Word вставляется кнопкой «Уравнение»
//! (формат OMML, Office Math Markup Language).
//!
//! Формула — дерево: дробь из числителя и знаменателя, степень из
//! основания и показателя и т. д. Каждая часть — список узлов
//! ([`MathList`]). Формула стоит в тексте абзаца одним знаком
//! [`crate::OBJECT_CHAR`], как рисунок.

use crate::RawXml;

/// Список узлов формулы по порядку (одна «ячейка» формулы: числитель,
/// показатель степени, содержимое скобок…).
pub type MathList = Vec<MathNode>;

/// Формула.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Math {
    /// Формула на отдельной строке (`m:oMathPara`, «Профессиональный»
    /// вид в Word с крупными знаками сумм и интегралов). Иначе — формула
    /// в строке текста (`m:oMath`).
    pub display: bool,
    /// Выравнивание отдельной формулы.
    pub align: MathAlign,
    /// Строки формулы. У формулы в строке текста — ровно одна; у отдельной
    /// формулы их может быть несколько (каждый `m:oMath` внутри
    /// `m:oMathPara` — своя строка).
    pub lines: Vec<MathList>,
    /// Исходный XML формулы из файла. Пока формулу не меняли, она
    /// записывается обратно ровно так, со всеми свойствами, которые
    /// программа не понимает. После правки — `None`, и формула
    /// записывается по дереву [`Math::lines`].
    pub raw: Option<RawXml>,
}

/// Выравнивание отдельной формулы (`m:jc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MathAlign {
    /// По центру (как в Word по умолчанию; `center` и `centerGroup`).
    #[default]
    Center,
    /// По левому краю.
    Left,
    /// По правому краю.
    Right,
}

/// Вид дроби (`m:type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FractionKind {
    /// Числитель над знаменателем, между ними черта.
    #[default]
    Bar,
    /// Без черты (как биномиальный коэффициент без скобок).
    NoBar,
    /// Наклонная: числитель вверху слева, знаменатель внизу справа.
    Skewed,
    /// В строку: «a/b».
    Linear,
}

/// Узел формулы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathNode {
    /// Текст формулы (`m:r`): буквы, цифры, знаки действий.
    Text {
        /// Текст.
        text: String,
        /// Прямой текст (`m:sty m:val="p"` или `m:nor`): буквы не
        /// курсивом. Так пишут имена функций (sin, log) и обычные слова.
        plain: bool,
    },
    /// Дробь (`m:f`).
    Fraction {
        /// Вид дроби.
        kind: FractionKind,
        /// Числитель.
        num: MathList,
        /// Знаменатель.
        den: MathList,
    },
    /// Индексы (`m:sSub`, `m:sSup`, `m:sSubSup`, `m:sPre`).
    Scripts {
        /// Основание.
        base: MathList,
        /// Нижний индекс.
        sub: Option<MathList>,
        /// Верхний индекс (степень).
        sup: Option<MathList>,
        /// Индексы слева от основания (`m:sPre`).
        pre: bool,
    },
    /// Корень (`m:rad`).
    Radical {
        /// Степень корня; `None` — квадратный (степень скрыта).
        degree: Option<MathList>,
        /// Подкоренное выражение.
        base: MathList,
    },
    /// Скобки (`m:d`).
    Delimiter {
        /// Открывающая скобка; пусто — без скобки.
        open: String,
        /// Закрывающая скобка; пусто — без скобки.
        close: String,
        /// Разделитель между частями.
        separator: String,
        /// Части внутри скобок (через разделитель).
        items: Vec<MathList>,
    },
    /// Большой оператор: сумма, произведение, интеграл (`m:nary`).
    Nary {
        /// Знак оператора (∑, ∏, ∫…).
        op: String,
        /// Пределы над и под знаком (`true`), справа как индексы
        /// (`false`) или как принято по умолчанию (`None`: у интегралов
        /// справа, у остальных — над и под знаком).
        limits_over: Option<bool>,
        /// Нижний предел; `None` — скрыт.
        sub: Option<MathList>,
        /// Верхний предел; `None` — скрыт.
        sup: Option<MathList>,
        /// Выражение под оператором.
        base: MathList,
    },
    /// Функция (`m:func`): имя (sin, log) и аргумент.
    Function {
        /// Имя функции.
        name: MathList,
        /// Аргумент.
        base: MathList,
    },
    /// Знак над выражением: крышка, тильда, стрелка (`m:acc`).
    Accent {
        /// Знак (комбинируемый знак Юникода, например U+0302).
        mark: String,
        /// Выражение под знаком.
        base: MathList,
    },
    /// Черта над или под выражением (`m:bar`).
    Bar {
        /// Черта сверху; иначе снизу.
        top: bool,
        /// Выражение.
        base: MathList,
    },
    /// Предел под или над выражением (`m:limLow`, `m:limUpp`), например
    /// «lim» с «n→∞» снизу.
    Limit {
        /// Предел сверху (`m:limUpp`); иначе снизу.
        upper: bool,
        /// Основание.
        base: MathList,
        /// Предел.
        limit: MathList,
    },
    /// Фигурная скобка или другой знак над или под выражением
    /// (`m:groupChr`).
    GroupChar {
        /// Знак (по умолчанию нижняя фигурная скобка U+23DF).
        mark: String,
        /// Знак сверху; иначе снизу.
        top: bool,
        /// Выражение.
        base: MathList,
    },
    /// Матрица (`m:m`): строки из ячеек.
    Matrix(Vec<Vec<MathList>>),
    /// Система уравнений — строки друг под другом (`m:eqArr`).
    EqArray(Vec<MathList>),
    /// Группа без своего вида (`m:box`, `m:borderBox`, `m:phant`
    /// и непонятые элементы): показывается её содержимое.
    Group(MathList),
}

impl Math {
    /// Формула в строке текста из готовых узлов.
    pub fn inline(nodes: MathList) -> Math {
        Math {
            lines: vec![nodes],
            ..Math::default()
        }
    }

    /// Формула в линейном виде, как в Word «Линейный формат» (близко
    /// к UnicodeMath): `a/b`, `x^2`, `√(x)`. Нужен для простого текста,
    /// HTML и других форматов без формул. Строки отдельной формулы —
    /// через перевод строки.
    pub fn linear_text(&self) -> String {
        let lines: Vec<String> = self.lines.iter().map(|l| linear(l)).collect();
        lines.join("\n")
    }
}

/// Линейный вид списка узлов.
pub fn linear(list: &[MathNode]) -> String {
    list.iter().map(MathNode::linear_text).collect()
}

/// Линейный вид операнда: одиночный знак как есть, иначе в скобках.
fn operand(list: &[MathNode]) -> String {
    let text = linear(list);
    if text.chars().count() <= 1 || is_bracketed(list) {
        text
    } else {
        format!("({text})")
    }
}

fn is_bracketed(list: &[MathNode]) -> bool {
    matches!(list, [MathNode::Delimiter { open, .. }] if !open.is_empty())
}

impl MathNode {
    /// Узел с текстом курсивом (обычные буквы формулы).
    pub fn text(text: impl Into<String>) -> MathNode {
        MathNode::Text {
            text: text.into(),
            plain: false,
        }
    }

    /// Линейный вид узла (см. [`Math::linear_text`]).
    pub fn linear_text(&self) -> String {
        match self {
            MathNode::Text { text, .. } => text.clone(),
            MathNode::Fraction { kind, num, den } => match kind {
                FractionKind::NoBar => format!("{}¦{}", operand(num), operand(den)),
                _ => format!("{}/{}", operand(num), operand(den)),
            },
            MathNode::Scripts {
                base,
                sub,
                sup,
                pre,
            } => {
                let mut scripts = String::new();
                if let Some(sub) = sub {
                    scripts.push('_');
                    scripts.push_str(&operand(sub));
                }
                if let Some(sup) = sup {
                    scripts.push('^');
                    scripts.push_str(&operand(sup));
                }
                if *pre {
                    format!("{scripts} {}", linear(base))
                } else {
                    format!("{}{scripts}", operand(base))
                }
            }
            MathNode::Radical { degree, base } => match degree {
                Some(degree) => format!("√({}&{})", linear(degree), linear(base)),
                None => format!("√{}", operand(base)),
            },
            MathNode::Delimiter {
                open,
                close,
                separator,
                items,
            } => {
                let inner: Vec<String> = items.iter().map(|i| linear(i)).collect();
                // Как в Word: разделитель «|» — знак `│`, пустая сторона
                // при непустой другой — `├` или `┤`.
                let separator = if separator == "|" { "│" } else { separator };
                let (open, close) = match (open.is_empty(), close.is_empty()) {
                    (true, false) => ("├", close.as_str()),
                    (false, true) => (open.as_str(), "┤"),
                    _ => (open.as_str(), close.as_str()),
                };
                format!("{open}{}{close}", inner.join(separator))
            }
            MathNode::Nary {
                op, sub, sup, base, ..
            } => {
                let mut out = op.clone();
                if let Some(sub) = sub {
                    out.push('_');
                    out.push_str(&operand(sub));
                }
                if let Some(sup) = sup {
                    out.push('^');
                    out.push_str(&operand(sup));
                }
                out.push('▒');
                out.push_str(&operand(base));
                out
            }
            MathNode::Function { name, base } => format!("{}⁡{}", linear(name), operand(base)),
            MathNode::Accent { mark, base } => format!("{}{mark}", operand(base)),
            MathNode::Bar { top: true, base } => format!("¯{}", operand(base)),
            MathNode::Bar { top: false, base } => format!("▁{}", operand(base)),
            MathNode::Limit { upper, base, limit } => {
                let mark = if *upper { '┴' } else { '┬' };
                format!("{}{mark}{}", operand(base), operand(limit))
            }
            MathNode::GroupChar { mark, top, base } => {
                let side = if *top { '┴' } else { '┬' };
                format!("{mark}{side}{}", operand(base))
            }
            MathNode::Matrix(rows) => {
                let rows: Vec<String> = rows
                    .iter()
                    .map(|row| row.iter().map(|c| linear(c)).collect::<Vec<_>>().join("&"))
                    .collect();
                format!("■({})", rows.join("@"))
            }
            MathNode::EqArray(rows) => {
                let rows: Vec<String> = rows.iter().map(|r| linear(r)).collect();
                format!("█({})", rows.join("@"))
            }
            MathNode::Group(list) => linear(list),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> MathList {
        vec![MathNode::text(s)]
    }

    #[test]
    fn linear_forms() {
        let frac = MathNode::Fraction {
            kind: FractionKind::Bar,
            num: t("a+b"),
            den: t("c"),
        };
        assert_eq!(frac.linear_text(), "(a+b)/c");
        let power = MathNode::Scripts {
            base: t("x"),
            sub: None,
            sup: Some(t("2")),
            pre: false,
        };
        assert_eq!(power.linear_text(), "x^2");
        let root = MathNode::Radical {
            degree: None,
            base: t("x"),
        };
        assert_eq!(root.linear_text(), "√x");
        let sum = MathNode::Nary {
            op: "∑".into(),
            limits_over: None,
            sub: Some(t("i=1")),
            sup: Some(t("n")),
            base: t("i"),
        };
        assert_eq!(sum.linear_text(), "∑_(i=1)^n▒i");
        let math = Math::inline(vec![MathNode::text("y="), power]);
        assert_eq!(math.linear_text(), "y=x^2");
    }
}
