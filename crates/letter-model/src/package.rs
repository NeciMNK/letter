//! Части исходного файла, которые программа не разбирает.

/// Содержимое исходного файла (пакета OOXML), которое нужно вернуть
/// на место при сохранении: картинки, колонтитулы, сноски, нумерация,
/// настройки и всё остальное, что программа пока не понимает.
///
/// Части, которые программа строит заново из модели (главная часть
/// документа, стили, свойства документа), здесь не хранятся.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourcePackage {
    /// Имя главной части документа (обычно `word/document.xml`).
    /// При сохранении документ записывается под тем же именем, чтобы
    /// связи из других частей остались верными.
    pub main_part: String,
    /// Имя части стилей, если стили были прочитаны из неё. Если часть
    /// стилей есть, но испорчена, её байты лежат в [`SourcePackage::parts`].
    pub styles_part: Option<String>,
    /// Имя части списков (`word/numbering.xml`), если списки были
    /// прочитаны из неё.
    pub numbering_part: Option<String>,
    /// Атрибуты корневого элемента части списков.
    pub numbering_root_attributes: Vec<(String, String)>,
    /// Имя части основных свойств (`docProps/core.xml`), если они были
    /// прочитаны из неё.
    pub core_part: Option<String>,
    /// Имя части сведений о программе (`docProps/app.xml`), если они были
    /// прочитаны из неё.
    pub app_part: Option<String>,
    /// Атрибуты корневого элемента главной части: объявления пространств
    /// имён и `mc:Ignorable` — пары «имя атрибута — значение».
    pub main_root_attributes: Vec<(String, String)>,
    /// То же для корневого элемента части стилей.
    pub styles_root_attributes: Vec<(String, String)>,
    /// Типы содержимого по расширению имени (`Default` в `[Content_Types].xml`):
    /// пары «расширение — тип».
    pub default_content_types: Vec<(String, String)>,
    /// Типы содержимого отдельных частей (`Override`): пары «имя части — тип».
    pub content_types: Vec<(String, String)>,
    /// Остальные части в порядке их следования в исходном файле.
    pub parts: Vec<SourcePart>,
}

/// Одна часть пакета: имя внутри архива и байты без изменений.
#[derive(Clone, PartialEq, Eq)]
pub struct SourcePart {
    /// Имя части без ведущего `/` (например, `word/media/image1.png`).
    pub name: String,
    /// Содержимое части.
    pub data: Vec<u8>,
}

impl std::fmt::Debug for SourcePart {
    /// Байты не печатаем: картинки бывают по несколько мегабайт.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourcePart")
            .field("name", &self.name)
            .field("len", &self.data.len())
            .finish()
    }
}

impl SourcePackage {
    /// Часть по имени (без учёта регистра букв, как принято в OOXML).
    pub fn part(&self, name: &str) -> Option<&SourcePart> {
        self.parts
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_part_ignoring_case() {
        let package = SourcePackage {
            parts: vec![SourcePart {
                name: "word/Media/a.png".into(),
                data: vec![1, 2, 3],
            }],
            ..SourcePackage::default()
        };
        assert_eq!(
            package.part("word/media/A.PNG").map(|p| p.data.len()),
            Some(3)
        );
        assert!(package.part("word/media/b.png").is_none());
        assert_eq!(
            format!("{:?}", package.parts[0]),
            r#"SourcePart { name: "word/Media/a.png", len: 3 }"#
        );
    }
}
