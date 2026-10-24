//! Защита документа: ограничение правки (`w:documentProtection`)
//! и пароль на изменение файла (`w:writeProtection`).
//!
//! Пароль в файле не хранится: только его хеш с «солью» (случайными
//! байтами) и числом повторов. Проверка и вычисление хеша —
//! в `letter-editor`, модель только хранит значения.

/// Какую правку разрешает защита документа (`w:edit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditRestriction {
    /// Без ограничений (`none`).
    #[default]
    None,
    /// «Только чтение» (`readOnly`): менять ничего нельзя.
    ReadOnly,
    /// «Примечания» (`comments`): можно только добавлять примечания.
    Comments,
    /// «Запись исправлений» (`trackedChanges`): правка только
    /// с записью исправлений.
    TrackedChanges,
    /// «Ввод данных в поля форм» (`forms`).
    Forms,
}

impl EditRestriction {
    /// Значение атрибута `w:edit` в файле.
    pub fn as_str(self) -> &'static str {
        match self {
            EditRestriction::None => "none",
            EditRestriction::ReadOnly => "readOnly",
            EditRestriction::Comments => "comments",
            EditRestriction::TrackedChanges => "trackedChanges",
            EditRestriction::Forms => "forms",
        }
    }

    /// Разбирает значение `w:edit`; неизвестное — `None`.
    pub fn parse(value: &str) -> Option<EditRestriction> {
        Some(match value.trim() {
            "none" => EditRestriction::None,
            "readOnly" => EditRestriction::ReadOnly,
            "comments" => EditRestriction::Comments,
            "trackedChanges" => EditRestriction::TrackedChanges,
            "forms" => EditRestriction::Forms,
            _ => return None,
        })
    }
}

/// Хеш пароля, как его пишет Word 2013 и новее: `hash = H(salt +
/// пароль в UTF-16LE)`, затем `spin_count` раз `hash = H(hash + номер
/// повтора)`. Хеш и соль — в Base64.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordHash {
    /// Алгоритм (`w:algorithmName`), например `SHA-512`.
    pub algorithm: String,
    /// Хеш (`w:hashValue`), Base64.
    pub hash: String,
    /// Соль (`w:saltValue`), Base64.
    pub salt: String,
    /// Число повторов (`w:spinCount`).
    pub spin_count: u32,
}

/// Ограничение правки документа (`w:documentProtection`), окно
/// «Ограничить редактирование» в Word.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocumentProtection {
    /// Что разрешено.
    pub edit: EditRestriction,
    /// Защита включена (`w:enforcement`). Если нет — ограничение
    /// только записано, но не действует.
    pub enforced: bool,
    /// Пароль на снятие защиты; `None` — снимается без пароля.
    pub password: Option<PasswordHash>,
    /// Прочие атрибуты элемента (например, хеш старого вида Word 2007:
    /// `w:cryptProviderType`, `w:hash`, `w:salt`), локальное имя
    /// и значение — возвращаются в файл как были.
    pub other: Vec<(String, String)>,
}

impl DocumentProtection {
    /// Защита действует и не даёт править текст как обычно: всё, кроме
    /// «Запись исправлений» (там правка разрешена, но записывается).
    pub fn locks_editing(&self) -> bool {
        self.enforced
            && !matches!(
                self.edit,
                EditRestriction::None | EditRestriction::TrackedChanges
            )
    }
}

/// Пароль на изменение файла и «рекомендовать доступ только для чтения»
/// (`w:writeProtection`): при открытии Word предлагает ввести пароль
/// или открыть документ только для чтения.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WriteProtection {
    /// «Рекомендовать доступ только для чтения» (`w:recommended`).
    pub recommended: bool,
    /// Пароль на изменение; `None` — без пароля.
    pub password: Option<PasswordHash>,
    /// Прочие атрибуты (старый вид хеша и т. п.), как в
    /// [`DocumentProtection::other`].
    pub other: Vec<(String, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restriction_names_round_trip() {
        for kind in [
            EditRestriction::None,
            EditRestriction::ReadOnly,
            EditRestriction::Comments,
            EditRestriction::TrackedChanges,
            EditRestriction::Forms,
        ] {
            assert_eq!(EditRestriction::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(EditRestriction::parse("other"), None);
    }

    #[test]
    fn only_enforced_restrictions_lock() {
        let mut p = DocumentProtection {
            edit: EditRestriction::ReadOnly,
            ..Default::default()
        };
        assert!(!p.locks_editing());
        p.enforced = true;
        assert!(p.locks_editing());
        p.edit = EditRestriction::TrackedChanges;
        assert!(!p.locks_editing());
    }
}
