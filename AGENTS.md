# Инструкции для нейросетей и новых участников

Главный документ проекта — [plan.md](plan.md). Прочитай его целиком перед
работой.

Кратко:

- `letter` — настольный (без веба) редактор на Rust, копия Microsoft Word по
  виду, функциям и совместимости с .docx/.doc. GUI — `egui`/`eframe`.
- Владелец — Nikita (GitHub `bikhe`), опыта программирования нет, весь код
  пишут нейросети. Общаться на русском, объяснять простыми словами.
- Интерфейс делаем похожим на Word, но не копируем иконки, логотипы, шрифты
  и тексты Microsoft.
- Как брать задачу: первая незакрытая `[ ]` в текущей фазе `plan.md` с
  выполненными зависимостями. Отдельная ветка, PR; в том же PR отметить
  задачу `[x]`.
- Один PR — одна задача (или несколько очень мелких из одной фазы).
- Должны проходить `cargo build`, `cargo test`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`.
- Новая функция — только с тестом. Для форматов — тест «открыть → сохранить
  → открыть».
- Никаких `unwrap()` в коде чтения файлов. Публичное API документируется `///`.
- Тексты интерфейса — только через `i18n::tr("имя")`; новое имя добавлять
  сразу в `crates/letter-app/assets/i18n/ru.ftl` и `en.ftl`.
- Спорные решения (крупная зависимость, смена архитектуры) — описать варианты
  в PR и спросить владельца; принятые записать в «Журнал решений» в `plan.md`.

## Структура

- `crates/letter-model` — модель документа.
- `crates/letter-docx` — чтение/запись .docx.
- `crates/letter-doc`, `crates/letter-rtf` — другие форматы (.doc позже, RTF готов).
- `crates/letter-text` — TXT, HTML, Markdown.
- `crates/letter-odt` — ODT (OpenDocument, LibreOffice).
- `crates/letter-layout` — вёрстка.
- `crates/letter-render` — отрисовка.
- `crates/letter-editor` — курсор, команды, отмена.
- `crates/letter-app` — окно и интерфейс, бинарник `letter`.
- `crates/letter-cli` — консольная утилита для отладки: `dump`, `roundtrip`, `pages`,
  `render`, `compare` (сверка вёрстки с PDF из Word), `pdf-lines`, `pdf` (сохранить в PDF).
- `tests/fixtures/reference/` — эталонные PDF для сверки вёрстки и
  `baseline.txt`; если вёрстка стала лучше, обнови его
  (`LETTER_UPDATE_BASELINE=1 cargo test -p letter-cli --test compare`).
- `tests/fixtures/` — тестовые документы (источники в `tests/fixtures/README.md`).
- `docs/` — руководство пользователя (mdBook, сайт на GitHub Pages). Новая
  функция для пользователя — описать и здесь; сочетания клавиш сверяет тест
  `user_guide_lists_every_shortcut`.
