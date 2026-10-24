//! Разновидности файла Word в формате Open XML: обычный документ (.docx),
//! документ с макросами (.docm), шаблон (.dotx) и шаблон с макросами
//! (.dotm). Внутри они устроены одинаково, отличаются только типом
//! содержимого главной части и тем, можно ли хранить в них макросы.

use letter_model::Document;

/// Разновидность файла Word Open XML.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FileKind {
    /// Обычный документ (.docx). Макросов в нём быть не может.
    #[default]
    Document,
    /// Документ с макросами (.docm).
    MacroDocument,
    /// Шаблон (.dotx). Макросов в нём быть не может.
    Template,
    /// Шаблон с макросами (.dotm).
    MacroTemplate,
}

impl FileKind {
    /// Все разновидности.
    pub const ALL: [FileKind; 4] = [
        FileKind::Document,
        FileKind::MacroDocument,
        FileKind::Template,
        FileKind::MacroTemplate,
    ];

    /// Разновидность по расширению файла без точки (`docx`, `DOCM`…).
    pub fn from_extension(extension: &str) -> Option<FileKind> {
        FileKind::ALL
            .into_iter()
            .find(|k| k.extension().eq_ignore_ascii_case(extension))
    }

    /// Разновидность по пути к файлу (по его расширению).
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Option<FileKind> {
        FileKind::from_extension(path.as_ref().extension()?.to_str()?)
    }

    /// Разновидность по типу содержимого главной части.
    pub fn from_content_type(content_type: &str) -> Option<FileKind> {
        FileKind::ALL
            .into_iter()
            .find(|k| k.content_type() == content_type)
    }

    /// Расширение файла без точки.
    pub fn extension(self) -> &'static str {
        match self {
            FileKind::Document => "docx",
            FileKind::MacroDocument => "docm",
            FileKind::Template => "dotx",
            FileKind::MacroTemplate => "dotm",
        }
    }

    /// Тип содержимого главной части (`[Content_Types].xml`). Word
    /// проверяет, что он подходит к расширению, и иначе не открывает файл.
    pub fn content_type(self) -> &'static str {
        match self {
            FileKind::Document => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
            }
            FileKind::MacroDocument => "application/vnd.ms-word.document.macroEnabled.main+xml",
            FileKind::Template => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml"
            }
            FileKind::MacroTemplate => {
                "application/vnd.ms-word.template.macroEnabledTemplate.main+xml"
            }
        }
    }

    /// Можно ли хранить в файле макросы.
    pub fn has_macros(self) -> bool {
        matches!(self, FileKind::MacroDocument | FileKind::MacroTemplate)
    }

    /// Шаблон ли это. Word открывает шаблон как новый документ на его
    /// основе.
    pub fn is_template(self) -> bool {
        matches!(self, FileKind::Template | FileKind::MacroTemplate)
    }

    /// Та же разновидность, но обычный документ или шаблон: с макросами
    /// или без, как у `self`.
    pub fn as_document(self) -> FileKind {
        if self.has_macros() {
            FileKind::MacroDocument
        } else {
            FileKind::Document
        }
    }
}

/// Разновидность, из которой документ был прочитан (по типу содержимого
/// его главной части). Новый документ и документ из другого формата —
/// [`FileKind::Document`].
pub fn kind_of(doc: &Document) -> FileKind {
    let Some(source) = &doc.source else {
        return FileKind::Document;
    };
    source
        .content_types
        .iter()
        .find(|(part, _)| part.eq_ignore_ascii_case(&source.main_part))
        .and_then(|(_, content_type)| FileKind::from_content_type(content_type))
        .unwrap_or_default()
}

/// Есть ли в документе макросы (проект VBA), которые сохранятся только
/// в файле с макросами (.docm, .dotm). Программа макросы не выполняет,
/// а только бережно переносит.
pub fn has_macros(doc: &Document) -> bool {
    doc.source.as_ref().is_some_and(|source| {
        source.parts.iter().any(|part| {
            crate::write::content_type_of_part(source, &part.name)
                .is_some_and(|t| t == CT_VBA_PROJECT)
        })
    })
}

/// Тип содержимого проекта VBA (`word/vbaProject.bin`).
pub(crate) const CT_VBA_PROJECT: &str = "application/vnd.ms-office.vbaProject";

/// Типы содержимого частей, которые относятся к макросам: при записи
/// в файл без макросов они выбрасываются, как это делает Word.
pub(crate) const MACRO_CONTENT_TYPES: &[&str] = &[
    CT_VBA_PROJECT,
    "application/vnd.ms-word.vbaData+xml",
    "application/vnd.ms-word.keyMapCustomizations+xml",
    "application/vnd.ms-word.attachedToolbars",
];

/// Окончания типов связей с частями макросов.
pub(crate) const MACRO_RELATIONSHIPS: &[&str] = &[
    "/vbaProject",
    "/wordVbaData",
    "/keyMapCustomizations",
    "/attachedToolbars",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension_and_content_type() {
        for kind in FileKind::ALL {
            assert_eq!(FileKind::from_extension(kind.extension()), Some(kind));
            assert_eq!(FileKind::from_content_type(kind.content_type()), Some(kind));
        }
        assert_eq!(
            FileKind::from_extension("DOTM"),
            Some(FileKind::MacroTemplate)
        );
        assert_eq!(
            FileKind::from_path("a/b.Docm"),
            Some(FileKind::MacroDocument)
        );
        assert_eq!(FileKind::from_extension("rtf"), None);
        assert!(FileKind::Template.is_template() && !FileKind::Template.has_macros());
        assert_eq!(
            FileKind::MacroTemplate.as_document(),
            FileKind::MacroDocument
        );
        assert_eq!(FileKind::Template.as_document(), FileKind::Document);
    }

    #[test]
    fn new_document_is_a_plain_document() {
        assert_eq!(kind_of(&Document::default()), FileKind::Document);
        assert!(!has_macros(&Document::default()));
    }
}
