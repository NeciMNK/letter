//! Разбор линейного вида формулы — «Линейного формата» Word (близко
//! к UnicodeMath): `a/b`, `x^2`, `√(x)`, `∑_(i=1)^n▒i`. Обратное
//! к [`Math::linear_text`]: так формула набирается в редакторе формул.
//!
//! Правила, как в Word:
//! - операнд — подряд идущие буквы и цифры или выражение в круглых
//!   скобках (скобки вокруг числителя, знаменателя, индекса и корня
//!   убираются: `(a+b)/c` — дробь с числителем `a+b`);
//! - индексы (`_`, `^`) связывают сильнее дроби: `x^2/y` — дробь;
//! - пробелы только разделяют части и в формулу не попадают;
//! - `\имя` заменяется знаком: `\alpha` → α, `\sum` → ∑, `\infty` → ∞.

use crate::math::{FractionKind, Math, MathList, MathNode};

/// Знаки больших операторов (сумма, произведение, интегралы…).
pub const NARY_OPERATORS: &str = "∑∏∐∫∬∭∮∯∰⋀⋁⋂⋃";

/// Имена функций, которые пишутся прямым шрифтом и берут аргумент.
const FUNCTIONS: [&str; 29] = [
    "sin", "cos", "tan", "cot", "sec", "csc", "sinh", "cosh", "tanh", "coth", "arcsin", "arccos",
    "arctan", "arccot", "log", "ln", "lg", "exp", "lim", "min", "max", "det", "dim", "gcd", "deg",
    "arg", "ker", "sup", "inf",
];

/// Открывающие скобки и их пары по умолчанию.
const OPEN: [(char, char); 7] = [
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('⟨', '⟩'),
    ('⌊', '⌋'),
    ('⌈', '⌉'),
    ('├', '┤'),
];

/// Закрывающие скобки (любая закрывает любую открывающую: `[0,1)`).
const CLOSE: &str = ")]}⟩⌋⌉┤";

/// Знак применения функции (U+2061): `sin⁡x`.
const FUNCTION_APPLY: char = '\u{2061}';

/// Знаки `\имя` в линейном виде (часть «Автозамены формул» Word).
const KEYWORDS: [(&str, &str); 70] = [
    ("alpha", "α"),
    ("beta", "β"),
    ("gamma", "γ"),
    ("delta", "δ"),
    ("epsilon", "ϵ"),
    ("varepsilon", "ε"),
    ("zeta", "ζ"),
    ("eta", "η"),
    ("theta", "θ"),
    ("iota", "ι"),
    ("kappa", "κ"),
    ("lambda", "λ"),
    ("mu", "μ"),
    ("nu", "ν"),
    ("xi", "ξ"),
    ("pi", "π"),
    ("rho", "ρ"),
    ("sigma", "σ"),
    ("tau", "τ"),
    ("phi", "ϕ"),
    ("varphi", "φ"),
    ("chi", "χ"),
    ("psi", "ψ"),
    ("omega", "ω"),
    ("Gamma", "Γ"),
    ("Delta", "Δ"),
    ("Theta", "Θ"),
    ("Lambda", "Λ"),
    ("Xi", "Ξ"),
    ("Pi", "Π"),
    ("Sigma", "Σ"),
    ("Phi", "Φ"),
    ("Psi", "Ψ"),
    ("Omega", "Ω"),
    ("infty", "∞"),
    ("partial", "∂"),
    ("nabla", "∇"),
    ("pm", "±"),
    ("mp", "∓"),
    ("times", "×"),
    ("div", "÷"),
    ("cdot", "⋅"),
    ("le", "≤"),
    ("ge", "≥"),
    ("ne", "≠"),
    ("approx", "≈"),
    ("equiv", "≡"),
    ("sim", "∼"),
    ("propto", "∝"),
    ("in", "∈"),
    ("notin", "∉"),
    ("subset", "⊂"),
    ("cup", "∪"),
    ("cap", "∩"),
    ("forall", "∀"),
    ("exists", "∃"),
    ("to", "→"),
    ("rightarrow", "→"),
    ("leftarrow", "←"),
    ("Rightarrow", "⇒"),
    ("Leftrightarrow", "⇔"),
    ("sqrt", "√"),
    ("cbrt", "∛"),
    ("sum", "∑"),
    ("prod", "∏"),
    ("int", "∫"),
    ("iint", "∬"),
    ("oint", "∮"),
    ("matrix", "■"),
    ("eqarray", "█"),
];

impl Math {
    /// Формула из линейного вида (см. [`parse_linear`]). Строки — через
    /// перевод строки; `display` — формула на отдельной строке.
    pub fn from_linear(text: &str, display: bool) -> Math {
        let mut lines: Vec<MathList> = text.lines().map(parse_linear).collect();
        if lines.is_empty() || !display {
            lines = vec![lines.into_iter().flatten().collect()];
        }
        Math {
            display,
            lines,
            ..Math::default()
        }
    }
}

/// Разбирает одну строку линейного вида формулы в список узлов.
pub fn parse_linear(text: &str) -> MathList {
    let mut parser = Parser {
        chars: replace_keywords(text).chars().collect(),
        pos: 0,
    };
    let mut out = parser.sequence(&[]);
    // Лишние закрывающие скобки и разделители — просто знаки.
    while let Some(c) = parser.next() {
        out.push(MathNode::text(c.to_string()));
        out.extend(parser.sequence(&[]));
    }
    merge_text(out)
}

/// Заменяет `\имя` знаками; пробел сразу после имени съедается, как
/// в Word.
fn replace_keywords(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let len = after
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(after.len());
        let name = &after[..len];
        match KEYWORDS.iter().find(|(k, _)| *k == name) {
            Some((_, symbol)) => {
                out.push_str(symbol);
                rest = &after[len..];
                rest = rest.strip_prefix(' ').unwrap_or(rest);
            }
            None => {
                out.push('\\');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Автозамена при наборе: заменяет знаками только законченные `\имя`
/// (за которыми уже набран не буквенный знак; пробел после имени
/// съедается), чтобы `\in` не превратилось в ∈ на пути к `\infty`.
pub fn autocorrect_linear(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let len = after
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(after.len());
        let name = &after[..len];
        let finished = len < after.len();
        match KEYWORDS.iter().find(|(k, _)| *k == name) {
            Some((_, symbol)) if finished => {
                out.push_str(symbol);
                rest = &after[len..];
                rest = rest.strip_prefix(' ').unwrap_or(rest);
            }
            _ => {
                out.push('\\');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Склеивает соседние куски текста одного вида.
fn merge_text(list: MathList) -> MathList {
    let mut out: MathList = Vec::with_capacity(list.len());
    for node in list {
        if let MathNode::Text { text, plain } = &node
            && let Some(MathNode::Text {
                text: last,
                plain: last_plain,
            }) = out.last_mut()
            && last_plain == plain
        {
            last.push_str(text);
            continue;
        }
        out.push(node);
    }
    out
}

/// Часть формулы до того, как стало понятно, операнд это или нет.
enum Term {
    /// Выражение в круглых скобках: как операнд — без скобок, в ряду —
    /// со скобками.
    Paren(MathList),
    /// Готовые узлы.
    Nodes(MathList),
}

impl Term {
    /// Как операнд дроби, индекса или корня.
    fn operand(self) -> MathList {
        match self {
            Term::Paren(list) | Term::Nodes(list) => merge_text(list),
        }
    }

    /// Как часть ряда.
    fn nodes(self) -> MathList {
        match self {
            Term::Paren(list) => vec![MathNode::Delimiter {
                open: "(".into(),
                close: ")".into(),
                separator: "|".into(),
                items: vec![merge_text(list)],
            }],
            Term::Nodes(list) => list,
        }
    }

    /// Имя функции (sin, log, lim) — прямым текстом.
    fn is_function_name(&self) -> bool {
        matches!(self, Term::Nodes(list) if matches!(
            list.as_slice(),
            [MathNode::Text { plain: true, text }] if FUNCTIONS.contains(&text.as_str())
        ))
    }
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    /// Съедает `c`, если он следующий.
    fn eat(&mut self, c: char) -> bool {
        let found = self.peek() == Some(c);
        if found {
            self.pos += 1;
        }
        found
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(' ') {
            self.pos += 1;
        }
    }

    /// Ряд частей до закрывающей скобки, одного из `stops` или конца.
    fn sequence(&mut self, stops: &[char]) -> MathList {
        let mut out = Vec::new();
        loop {
            self.skip_spaces();
            match self.peek() {
                None => break,
                Some(c) if stops.contains(&c) || CLOSE.contains(c) => break,
                Some(_) => {}
            }
            out.extend(self.unit(stops).nodes());
        }
        merge_text(out)
    }

    /// Часть с индексами и, если за ней `/`, дробь.
    fn unit(&mut self, stops: &[char]) -> Term {
        let term = self.scripted(stops);
        self.skip_spaces();
        let kind = match self.peek() {
            Some('/') => FractionKind::Bar,
            Some('¦') => FractionKind::NoBar,
            _ => return term,
        };
        self.pos += 1;
        self.skip_spaces();
        if self.at_stop(stops) {
            let mut nodes = term.nodes();
            let slash = if kind == FractionKind::Bar { "/" } else { "¦" };
            nodes.push(MathNode::text(slash));
            return Term::Nodes(nodes);
        }
        let den = self.scripted(stops);
        Term::Nodes(vec![MathNode::Fraction {
            kind,
            num: term.operand(),
            den: den.operand(),
        }])
    }

    /// Конец ряда: конец строки, закрывающая скобка или знак из `stops`.
    fn at_stop(&self, stops: &[char]) -> bool {
        match self.peek() {
            None => true,
            Some(c) => stops.contains(&c) || CLOSE.contains(c),
        }
    }

    /// Основание с надстрочными знаками, индексами, пределами и
    /// применением функции.
    fn scripted(&mut self, stops: &[char]) -> Term {
        let mut term = self.atom(stops);
        loop {
            match self.peek() {
                Some(c) if is_combining(c) => {
                    self.pos += 1;
                    term = Term::Nodes(vec![MathNode::Accent {
                        mark: c.to_string(),
                        base: term.operand(),
                    }]);
                }
                Some('_' | '^') => {
                    let (sub, sup) = self.scripts(stops);
                    term = Term::Nodes(vec![MathNode::Scripts {
                        base: term.nodes(),
                        sub,
                        sup,
                        pre: false,
                    }]);
                }
                Some(c @ ('┬' | '┴')) => {
                    self.pos += 1;
                    let limit = self.script_operand(stops);
                    term = Term::Nodes(vec![MathNode::Limit {
                        upper: c == '┴',
                        base: term.operand(),
                        limit,
                    }]);
                }
                Some(FUNCTION_APPLY) => {
                    self.pos += 1;
                    term = self.function(term, stops);
                }
                Some(' ' | '(') if term.is_function_name() => {
                    self.skip_spaces();
                    if self.at_stop(stops) {
                        return term;
                    }
                    term = self.function(term, stops);
                }
                _ => return term,
            }
        }
    }

    /// Функция с именем `name` и аргументом за ним.
    fn function(&mut self, name: Term, stops: &[char]) -> Term {
        self.skip_spaces();
        let base = self.scripted(stops).operand();
        Term::Nodes(vec![MathNode::Function {
            name: name.operand(),
            base,
        }])
    }

    /// Нижний и верхний индексы (`_` и `^` в любом порядке).
    fn scripts(&mut self, stops: &[char]) -> (Option<MathList>, Option<MathList>) {
        let (mut sub, mut sup) = (None, None);
        loop {
            if sub.is_none() && self.eat('_') {
                sub = Some(self.script_operand(stops));
            } else if sup.is_none() && self.eat('^') {
                sup = Some(self.script_operand(stops));
            } else {
                return (sub, sup);
            }
        }
    }

    /// Операнд индекса или предела: одна часть со знаками над ней.
    fn script_operand(&mut self, stops: &[char]) -> MathList {
        let mut term = self.atom(stops);
        while let Some(c) = self.peek().filter(|c| is_combining(*c)) {
            self.pos += 1;
            term = Term::Nodes(vec![MathNode::Accent {
                mark: c.to_string(),
                base: term.operand(),
            }]);
        }
        term.operand()
    }

    /// Одна часть без индексов.
    fn atom(&mut self, stops: &[char]) -> Term {
        let Some(c) = self.next() else {
            return Term::Nodes(Vec::new());
        };
        if c.is_alphanumeric() {
            let start = self.pos - 1;
            while self
                .peek()
                .is_some_and(|c| c.is_alphanumeric() || (c == '.' && self.digit_after_dot()))
            {
                self.pos += 1;
            }
            let text: String = self.chars[start..self.pos].iter().collect();
            let plain = FUNCTIONS.contains(&text.as_str());
            return Term::Nodes(vec![MathNode::Text { text, plain }]);
        }
        if let Some(&(_, close)) = OPEN.iter().find(|(open, _)| *open == c) {
            return self.delimiter(c, close);
        }
        match c {
            '|' | '‖' => self.symmetric(c, stops),
            '√' | '∛' | '∜' => self.radical(c, stops),
            '■' | '█' => self.array(c == '■', stops),
            '¯' | '▁' => Term::Nodes(vec![MathNode::Bar {
                top: c == '¯',
                base: self.script_operand(stops),
            }]),
            '_' | '^' => {
                // Индексы слева: `_a^b X`.
                self.pos -= 1;
                let (sub, sup) = self.scripts(stops);
                self.skip_spaces();
                let base = self.atom(stops).operand();
                Term::Nodes(vec![MathNode::Scripts {
                    base,
                    sub,
                    sup,
                    pre: true,
                }])
            }
            c if NARY_OPERATORS.contains(c) => self.nary(c, stops),
            c if "⏟⏞⏝⏜".contains(c) && matches!(self.peek(), Some('┬' | '┴')) => {
                let top = self.next() == Some('┴');
                Term::Nodes(vec![MathNode::GroupChar {
                    mark: c.to_string(),
                    top,
                    base: self.script_operand(stops),
                }])
            }
            c => Term::Nodes(vec![MathNode::text(c.to_string())]),
        }
    }

    /// Точка внутри числа (`3.14`), а не в конце.
    fn digit_after_dot(&self) -> bool {
        let before = self.pos.checked_sub(1).and_then(|i| self.chars.get(i));
        let after = self.chars.get(self.pos + 1);
        before.is_some_and(char::is_ascii_digit) && after.is_some_and(char::is_ascii_digit)
    }

    /// Скобки: от `open` до любой закрывающей. Части — через `│`.
    fn delimiter(&mut self, open: char, close: char) -> Term {
        let mut items = vec![self.sequence(&['│'])];
        while self.eat('│') {
            items.push(self.sequence(&['│']));
        }
        let end = match self.peek() {
            Some(c) if CLOSE.contains(c) => {
                self.pos += 1;
                c
            }
            _ => close,
        };
        if open == '(' && end == ')' && items.len() == 1 {
            return Term::Paren(items.pop().unwrap_or_default());
        }
        let side = |c: char| match c {
            '├' | '┤' => String::new(),
            c => c.to_string(),
        };
        Term::Nodes(vec![MathNode::Delimiter {
            open: side(open),
            close: side(end),
            separator: "|".into(),
            items,
        }])
    }

    /// Одинаковые скобки с двух сторон: `|x|`, `‖v‖`.
    fn symmetric(&mut self, bar: char, stops: &[char]) -> Term {
        let mut inner_stops = stops.to_vec();
        inner_stops.push(bar);
        let items = vec![self.sequence(&inner_stops)];
        if !self.eat(bar) {
            // Нет пары — просто знак.
            let mut nodes = vec![MathNode::text(bar.to_string())];
            nodes.extend(items.into_iter().flatten());
            return Term::Nodes(nodes);
        }
        Term::Nodes(vec![MathNode::Delimiter {
            open: bar.to_string(),
            close: bar.to_string(),
            separator: "|".into(),
            items,
        }])
    }

    /// Корень: `√x`, `√(x+1)`, `√(n&x)`, `∛x`.
    fn radical(&mut self, sign: char, stops: &[char]) -> Term {
        let mut degree = match sign {
            '∛' => Some(vec![MathNode::text("3")]),
            '∜' => Some(vec![MathNode::text("4")]),
            _ => None,
        };
        self.skip_spaces();
        let base = if self.eat('(') {
            let first = self.sequence(&['&']);
            let base = if self.eat('&') {
                degree = Some(first);
                self.sequence(&[])
            } else {
                first
            };
            self.eat(')');
            base
        } else {
            self.script_operand(stops)
        };
        Term::Nodes(vec![MathNode::Radical { degree, base }])
    }

    /// Матрица `■(a&b@c&d)` или система `█(x=1@y=2)`.
    fn array(&mut self, matrix: bool, stops: &[char]) -> Term {
        self.skip_spaces();
        if !self.eat('(') {
            let text = if matrix { "■" } else { "█" };
            let mut nodes = vec![MathNode::text(text)];
            nodes.extend(self.sequence(stops));
            return Term::Nodes(nodes);
        }
        let mut rows: Vec<Vec<MathList>> = Vec::new();
        loop {
            let mut row = vec![self.sequence(&['&', '@'])];
            while self.eat('&') {
                row.push(self.sequence(&['&', '@']));
            }
            rows.push(row);
            if !self.eat('@') {
                break;
            }
        }
        self.eat(')');
        let node = if matrix {
            MathNode::Matrix(rows)
        } else {
            MathNode::EqArray(rows.into_iter().map(|r| r.concat()).collect())
        };
        Term::Nodes(vec![node])
    }

    /// Большой оператор: `∑_(i=1)^n▒i`, `∫_0^1 x dx`.
    fn nary(&mut self, op: char, stops: &[char]) -> Term {
        let (sub, sup) = self.scripts(stops);
        self.skip_spaces();
        let base = if self.eat('▒') {
            self.skip_spaces();
            self.scripted(stops).operand()
        } else if self.at_stop(stops) {
            Vec::new()
        } else {
            self.scripted(stops).operand()
        };
        Term::Nodes(vec![MathNode::Nary {
            op: op.to_string(),
            limits_over: None,
            sub,
            sup,
            base,
        }])
    }
}

/// Комбинируемый знак над буквой (крышка, тильда, стрелка).
fn is_combining(c: char) -> bool {
    matches!(c, '\u{300}'..='\u{36f}' | '\u{20d0}'..='\u{20ff}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> MathList {
        vec![MathNode::text(s)]
    }

    fn round(text: &str) -> String {
        Math::from_linear(text, false).linear_text()
    }

    #[test]
    fn structures() {
        assert_eq!(
            parse_linear("(a+b)/c"),
            vec![MathNode::Fraction {
                kind: FractionKind::Bar,
                num: t("a+b"),
                den: t("c"),
            }]
        );
        assert_eq!(
            parse_linear("y=x^2"),
            vec![
                MathNode::text("y="),
                MathNode::Scripts {
                    base: t("x"),
                    sub: None,
                    sup: Some(t("2")),
                    pre: false,
                },
            ]
        );
        // Индекс связывает сильнее дроби.
        let fraction = parse_linear("x^2/y");
        let [MathNode::Fraction { num, .. }] = fraction.as_slice() else {
            panic!("ждали дробь");
        };
        assert!(matches!(num.as_slice(), [MathNode::Scripts { .. }]));
        // Скобки в ряду остаются скобками.
        assert!(matches!(
            parse_linear("2(a+b)").as_slice(),
            [MathNode::Text { .. }, MathNode::Delimiter { .. }]
        ));
        assert_eq!(
            parse_linear("√(3&x)"),
            vec![MathNode::Radical {
                degree: Some(t("3")),
                base: t("x"),
            }]
        );
        assert_eq!(
            parse_linear("sin x"),
            vec![MathNode::Function {
                name: vec![MathNode::Text {
                    text: "sin".into(),
                    plain: true,
                }],
                base: t("x"),
            }]
        );
        assert_eq!(
            parse_linear("■(1&0@0&1)"),
            vec![MathNode::Matrix(vec![
                vec![t("1"), t("0")],
                vec![t("0"), t("1")],
            ])]
        );
    }

    #[test]
    fn keywords_and_spaces() {
        assert_eq!(parse_linear("\\alpha +\\beta"), t("α+β"));
        assert_eq!(parse_linear("a \\unknown"), t("a\\unknown"));
        assert_eq!(
            parse_linear("\\sum_(i=1)^n i"),
            vec![MathNode::Nary {
                op: "∑".into(),
                limits_over: None,
                sub: Some(t("i=1")),
                sup: Some(t("n")),
                base: t("i"),
            }]
        );
        assert_eq!(parse_linear("3.14"), t("3.14"));
    }

    #[test]
    fn autocorrect_waits_for_the_end_of_a_name() {
        assert_eq!(autocorrect_linear("\\in"), "\\in");
        assert_eq!(autocorrect_linear("\\infty "), "∞");
        assert_eq!(autocorrect_linear("a\\pm b"), "a±b");
        assert_eq!(autocorrect_linear("\\sqrt("), "√(");
        assert_eq!(autocorrect_linear("\\foo "), "\\foo ");
    }

    #[test]
    fn linear_round_trip() {
        for text in [
            "(a+b)/c",
            "y=x^2",
            "x_1^2",
            "√x",
            "√(n&x+1)",
            "∑_(i=1)^n▒(i^2)",
            "∫_0^1▒(x^2)",
            "sin⁡x+cos⁡x",
            "(lim)┬(n→∞)⁡(a_n)",
            "|x|",
            "[0,1)",
            "■(a&b@c&d)",
            "█(x+y=1@x-y=0)",
            "a¦b",
            "¯x",
            "x̂",
            "{x│x>0}",
            "_a^b X",
        ] {
            assert_eq!(round(text), text, "{text}");
        }
    }

    #[test]
    fn broken_input_is_kept() {
        assert_eq!(round("a/"), "a/");
        assert_eq!(round("(a"), "(a)");
        assert_eq!(round("a)"), "a)");
        assert_eq!(round("√"), "√");
        assert_eq!(Math::from_linear("", true).lines, vec![Vec::new()]);
        let two = Math::from_linear("x=1\ny=2", true);
        assert_eq!(two.lines.len(), 2);
    }
}
