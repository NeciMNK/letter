# letter

Открытый текстовый редактор для Windows, macOS и Linux, написанный на Rust.
Цель — повторить внешний вид, возможности и совместимость с файлами
Microsoft Word (2024 и более ранних версий).

Проект в самом начале пути: сейчас программа открывает пустое окно.
План работ — в [plan.md](plan.md).

## Скачать готовую программу

Готовые сборки лежат на странице
[Releases](https://github.com/bikhe/letter/releases): выпуски с номером
версии и «Ночная сборка» — самая свежая, из ветки main.

- **Windows:** `.msi` или `-setup.exe` — установщик; `-windows-x64-portable.zip` —
  портативная версия: распакуйте куда угодно и запустите `letter.exe`.
- **macOS:** `.dmg` — установка; `-macos-portable.zip` — `letter.app` без установки.
- **Linux:** `.deb` — пакет для Ubuntu/Debian; `.AppImage` и
  `-linux-x64-portable.tar.gz` — запуск без установки.

Портативная версия хранит настройки в папке `letter-data` рядом с программой.
Сборки пока не подписаны, поэтому Windows и macOS при первом запуске
предупредят о неизвестном издателе.

## Как собрать и запустить

Понадобится около 2 ГБ свободного места и интернет (первая сборка скачивает
библиотеки).

### 1. Установите Rust

- **Windows:** скачайте и запустите `rustup-init.exe` с сайта
  <https://rustup.rs>. Если установщик попросит поставить
  «Visual Studio C++ Build Tools», согласитесь (нужен вариант
  «Desktop development with C++»).
- **macOS:** откройте «Терминал» и выполните:
  ```sh
  xcode-select --install
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **Linux:** в терминале выполните:
  ```sh
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
  Для Ubuntu/Debian также нужны пакеты:
  ```sh
  sudo apt install build-essential pkg-config libxkbcommon-x11-0 \
    libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
  ```

После установки закройте терминал и откройте его заново.

### 2. Скачайте проект

Нажмите на GitHub зелёную кнопку **Code → Download ZIP** и распакуйте архив,
или, если установлен git:

```sh
git clone https://github.com/bikhe/letter.git
cd letter
```

### 3. Запустите

В терминале, находясь в папке проекта:

```sh
cargo run -p letter-app
```

Первая сборка занимает несколько минут, следующие — секунды. Должно открыться
окно с заголовком «letter».

## Для разработчиков

Перед отправкой изменений должны проходить:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

То же самое проверяет CI на Windows, macOS и Linux. Правила работы и
архитектура описаны в [plan.md](plan.md).

Посмотреть, как программа поняла файл .docx, проверить сохранение
раскладку по страницам и нарисовать страницы в картинки PNG:

```sh
cargo run -p letter-cli -- dump документ.docx
cargo run -p letter-cli -- roundtrip документ.docx копия.docx
cargo run -p letter-cli -- pages документ.docx
cargo run -p letter-cli -- render документ.docx папка
cargo run -p letter-cli -- compare документ.docx эталон-из-word.pdf
```

Сверка вёрстки с Word по всем эталонам — `cargo test -p letter-cli --test
compare -- --nocapture` (подробнее в
[tests/fixtures/reference/README.md](tests/fixtures/reference/README.md)).

## Лицензия

[MIT](LICENSE). Проект не связан с Microsoft; «Microsoft Word» — товарный
знак Microsoft Corporation.
