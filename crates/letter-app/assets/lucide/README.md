# Значки

Набор [Lucide](https://lucide.dev) версии 1.61.0, шрифт `lucide.ttf` из
npm-пакета `lucide-static`. Лицензия ISC (свободная, нужно только сохранить
текст лицензии) — файл `LICENSE` рядом. Значки Microsoft не используем.

- `lucide.ttf` — шрифт: каждый значок — один символ из области U+E000…U+F8FF.
- `codes.txt` — имя значка и код символа (из `font/info.json` пакета).
  Найти нужный значок: поиск на https://lucide.dev/icons, затем имя — в
  `codes.txt`.

Как добавить значок: новый вариант в `enum Icon` в
`crates/letter-app/src/icons.rs` с именем и кодом из `codes.txt`; тест
`codes_match_the_font` проверит, что код верный.

Как обновить набор: скачать `npm pack lucide-static@<версия>`, заменить
`lucide.ttf` и `LICENSE`, пересоздать `codes.txt` из `font/info.json`
(коды между версиями могут меняться — тест это покажет).
