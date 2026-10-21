# Эталоны для сверки вёрстки

Здесь лежат PDF, с которыми сравнивается наша вёрстка (задача F2.8 в
[`plan.md`](../../../plan.md)). Для каждого `<папка>/<имя>.pdf` берётся
документ `../docx/<имя>.docx`, раскладывается по страницам и сравнивается:
совпадают ли переносы строк, страницы и положение каждой строки.

| Папка | Откуда PDF |
|---|---|
| `word/` | Сохранены из настоящего Microsoft Word — **главный эталон**. |
| `libreoffice/` | Временные: `soffice --headless --convert-to pdf` (LibreOffice 24.2). Нужны, пока нет PDF из Word. LibreOffice верстает не совсем как Word (например, лишнее место при множителе 1,15 ставит под строку, а не над ней, и берёт настоящие размеры Caladea вместо Cambria), поэтому подгонять вёрстку под эти файлы нельзя — только замечать, что что-то сломалось. |

## Как добавить эталон из Word (для владельца)

1. Откройте в Word файл из папки `tests/fixtures/docx`, например `pages.docx`.
2. «Файл → Сохранить как» (или «Экспорт»), тип файла — **PDF**. Имя оставьте
   тем же: `pages.pdf`. Оптимизация — «Стандартная».
3. Загрузите PDF на GitHub в папку `tests/fixtures/reference/word`:
   откройте её на сайте, «Add file → Upload files», перетащите файлы,
   «Commit changes».
4. Напишите в чат, какая у вас версия Word (например, «Word 2021, Windows»),
   — запишем её в таблицу ниже.

Ничего ломаться не должно: новый эталон тест просто покажет в отчёте.

| Файл в `word/` | Версия Word | Кто и когда добавил |
|---|---|---|
| все 10 файлов из `docx/` | Microsoft Word 2024 | владелец (kernel64), 2026-10-21 |

## Как смотреть результат

```sh
cargo test -p letter-cli --test compare -- --nocapture
```

Печатает таблицу по всем эталонам, полный отчёт со всеми расхождениями —
в `target/tmp/layout-compare.md`. Один файл:

```sh
cargo run -p letter-cli -- compare --embedded-fonts --tolerance 1 \
    tests/fixtures/docx/pages.docx tests/fixtures/reference/word/pages.pdf
cargo run -p letter-cli -- pdf-lines tests/fixtures/reference/word/pages.pdf
```

В `baseline.txt` записано, сколько строк каждого эталона уже совпадает.
Тест падает, только если стало меньше. Стало больше (или добавлен
эталон) — обновить файл:

```sh
LETTER_UPDATE_BASELINE=1 cargo test -p letter-cli --test compare
```

Допуск по умолчанию — 1 пт; другой: `LETTER_COMPARE_TOLERANCE=0.5`
(тогда только отчёт, без сравнения с `baseline.txt`).
