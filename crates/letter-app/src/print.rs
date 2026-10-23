//! Печать. Документ записывается во временный PDF (те же страницы, что
//! и на экране, см. [`letter_render::write_pdf`]) и отдаётся системе:
//!
//! - Linux и macOS — службе печати CUPS командой `lp`; список принтеров
//!   и принтер по умолчанию — из `lpstat`;
//! - Windows — программе, которая открывает PDF, командой «Печать на»
//!   (`Start-Process -Verb PrintTo`); список принтеров — `Get-Printer`.
//!
//! Копии и порядок страниц собираются прямо в PDF, поэтому от принтера
//! нужна только сама печать.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

/// Какие страницы печатать.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageChoice {
    /// Все.
    #[default]
    All,
    /// Страницу с курсором.
    Current,
    /// Перечисленные: «1-3, 5».
    Custom,
}

/// Задание печати.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintJob {
    /// Номера страниц (с нуля) по порядку печати, с копиями.
    pub pages: Vec<usize>,
    /// Принтер; `None` — принтер по умолчанию.
    pub printer: Option<String>,
}

/// Страницы из строки вида «1-3, 5, 8-» (номера с 1, как в Word; «8-» —
/// с восьмой до конца). `None`, если строка пустая или в ней ошибка:
/// не число, номер вне документа, диапазон наоборот.
pub fn parse_pages(text: &str, total: usize) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    for part in text.split([',', ';']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let number = |s: &str| -> Option<usize> {
            let n: usize = s.trim().parse().ok()?;
            (1..=total).contains(&n).then_some(n)
        };
        let (first, last) = match part.split_once(['-', '–']) {
            Some((a, b)) => {
                let first = number(a)?;
                let last = if b.trim().is_empty() {
                    total
                } else {
                    number(b)?
                };
                (first, last)
            }
            None => {
                let n = number(part)?;
                (n, n)
            }
        };
        if first > last {
            return None;
        }
        out.extend(first - 1..last);
    }
    (!out.is_empty()).then_some(out)
}

/// Порядок печати страниц `pages` в `copies` экземплярах: с разбором по
/// копиям — 1 2 3, 1 2 3; без — 1 1, 2 2, 3 3.
pub fn with_copies(pages: &[usize], copies: u32, collate: bool) -> Vec<usize> {
    let copies = copies.max(1) as usize;
    if collate {
        (0..copies).flat_map(|_| pages.iter().copied()).collect()
    } else {
        pages
            .iter()
            .flat_map(|&p| std::iter::repeat_n(p, copies))
            .collect()
    }
}

/// Принтеры системы: названия и принтер по умолчанию.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Printers {
    /// Названия.
    pub names: Vec<String>,
    /// Принтер по умолчанию.
    pub default: Option<String>,
}

/// Список принтеров, который ищется в фоне: опрос системы может занять
/// секунду, окно в это время не замирает.
#[derive(Debug, Clone, Default)]
pub struct PrinterList(Arc<Mutex<Option<Printers>>>);

impl PrinterList {
    /// Начинает поиск принтеров.
    pub fn start() -> PrinterList {
        let list = PrinterList::default();
        let slot = list.0.clone();
        std::thread::spawn(move || {
            let found = find_printers();
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(found);
            }
        });
        list
    }

    /// Найденные принтеры; `None` — ещё ищутся.
    pub fn get(&self) -> Option<Printers> {
        self.0.lock().ok().and_then(|p| p.clone())
    }
}

/// Опрашивает систему.
fn find_printers() -> Printers {
    if cfg!(windows) {
        let names = powershell("Get-Printer | Select-Object -ExpandProperty Name");
        let default = powershell(
            "Get-CimInstance Win32_Printer | Where-Object Default | \
             Select-Object -ExpandProperty Name",
        );
        Printers {
            names: lines(&names),
            default: lines(&default).into_iter().next(),
        }
    } else {
        let names = run("lpstat", &["-e"]);
        let default = run("lpstat", &["-d"]);
        Printers {
            names: lines(&names),
            default: default_from_lpstat(&default),
        }
    }
}

/// Непустые строки вывода команды.
fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Принтер по умолчанию из `lpstat -d`: «system default destination:
/// Имя» (или то же по-русски — после последнего двоеточия).
fn default_from_lpstat(text: &str) -> Option<String> {
    let line = text.lines().next()?;
    if !line.contains(':') {
        return None;
    }
    let name = line.rsplit(':').next()?.trim();
    (!name.is_empty() && !name.contains(' ')).then(|| name.to_owned())
}

fn run(program: &str, args: &[&str]) -> String {
    let mut command = Command::new(program);
    hide_window(&mut command);
    command
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn powershell(script: &str) -> String {
    run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )
}

/// Временный файл для очередного задания печати.
pub fn temp_pdf() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    std::env::temp_dir().join(format!("letter-print-{}-{stamp}.pdf", std::process::id()))
}

/// Отдаёт PDF `path` на печать. Ошибка — текст для пользователя.
pub fn send(path: &Path, printer: Option<&str>) -> Result<(), String> {
    let mut command = if cfg!(windows) {
        let file = quote_powershell(&path.to_string_lossy());
        let script = match printer {
            Some(printer) => format!(
                "Start-Process -FilePath {file} -Verb PrintTo -ArgumentList {}",
                quote_powershell(&format!("\"{printer}\""))
            ),
            None => format!("Start-Process -FilePath {file} -Verb Print"),
        };
        let mut command = Command::new("powershell");
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        command
    } else {
        let mut command = Command::new("lp");
        if let Some(printer) = printer {
            command.args(["-d", printer]);
        }
        command.arg("--").arg(path);
        command
    };
    hide_window(&mut command);
    let output = command.output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if error.is_empty() {
            output.status.to_string()
        } else {
            error
        })
    }
}

/// На Windows консольная программа без этого флага на миг показывает
/// чёрное окно.
#[cfg(windows)]
fn hide_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_window(_command: &mut Command) {}

/// Строка в одинарных кавычках PowerShell (кавычка внутри удваивается).
fn quote_powershell(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_ranges_like_word() {
        assert_eq!(parse_pages("1-3, 5", 6), Some(vec![0, 1, 2, 4]));
        assert_eq!(parse_pages("2", 2), Some(vec![1]));
        assert_eq!(parse_pages(" 4- ", 6), Some(vec![3, 4, 5]));
        assert_eq!(parse_pages("2–3;1", 3), Some(vec![1, 2, 0]));
        assert_eq!(parse_pages("", 3), None);
        assert_eq!(parse_pages("0", 3), None);
        assert_eq!(parse_pages("4", 3), None);
        assert_eq!(parse_pages("3-1", 3), None);
        assert_eq!(parse_pages("а", 3), None);
    }

    #[test]
    fn copies_collated_or_not() {
        assert_eq!(with_copies(&[0, 1], 2, true), [0, 1, 0, 1]);
        assert_eq!(with_copies(&[0, 1], 2, false), [0, 0, 1, 1]);
        assert_eq!(with_copies(&[2], 0, true), [2]);
    }

    #[test]
    fn default_printer_from_lpstat() {
        assert_eq!(
            default_from_lpstat("system default destination: Office_HP\n"),
            Some("Office_HP".into())
        );
        assert_eq!(
            default_from_lpstat("назначение по умолчанию: Kyocera"),
            Some("Kyocera".into())
        );
        assert_eq!(default_from_lpstat("no system default destination"), None);
        assert_eq!(lines(" a \n\nb\n"), ["a", "b"]);
    }

    #[test]
    fn powershell_quotes() {
        assert_eq!(quote_powershell("C:\\it's.pdf"), "'C:\\it''s.pdf'");
    }
}
