# Встроенные запасные шрифты

Открытые шрифты с теми же размерами букв, что и у шрифтов Microsoft.
Они встроены в программу, чтобы документ выглядел так же, как в Word,
даже если на компьютере нет оригинальных шрифтов.

| Файлы | Заменяет | Версия | Лицензия |
|---|---|---|---|
| `Carlito-*.ttf` | Calibri | 20230309 | SIL OFL 1.1, [LICENSE-Carlito.txt](LICENSE-Carlito.txt) |
| `Caladea-*.ttf` | Cambria | 1.002 (20130214) | Apache 2.0, [LICENSE-Caladea.txt](LICENSE-Caladea.txt) |
| `LiberationSerif-*.ttf` | Times New Roman | 2.1.5 | SIL OFL 1.1, [LICENSE-Liberation.txt](LICENSE-Liberation.txt) |
| `LiberationSans-*.ttf` | Arial | 2.1.5 | SIL OFL 1.1, [LICENSE-Liberation.txt](LICENSE-Liberation.txt) |
| `LiberationMono-*.ttf` | Courier New | 2.1.5 | SIL OFL 1.1, [LICENSE-Liberation.txt](LICENSE-Liberation.txt) |

Источник Carlito и Liberation: пакеты Debian `fonts-crosextra-carlito` и
`fonts-liberation` (исходники: <https://github.com/googlefonts/carlito>,
<https://github.com/liberationfonts/liberation-fonts>). Caladea — первый
выпуск для Chrome OS, `crosextrafonts-20130214.tar.gz`
(<https://commondatastorage.googleapis.com/chromeos-localmirror/distfiles/crosextrafonts-20130214.tar.gz>):
только в нём ширины букв и высота строки совпадают с Cambria (проверено
по PDF из Word 2024). В новых версиях
(<https://github.com/huertatipografica/Caladea>, пакет Debian 20200211)
буквы уже и цифры другие, текст переносится не как в Word. Кириллицы в
Caladea нет. Файлы не изменены.
