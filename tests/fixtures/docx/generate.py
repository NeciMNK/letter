"""Создаёт тестовые .docx файлы для letter.

Запуск (нужен Python 3 и библиотеки python-docx и Pillow):

    pip install python-docx pillow
    python3 tests/fixtures/docx/generate.py

Файлы появляются рядом с этим скриптом. Содержимое придумано нами,
поэтому файлы можно свободно использовать (лицензия проекта, MIT).
"""

import io
from pathlib import Path

from docx import Document
from docx.shared import Inches, Pt, RGBColor
from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent


def new_document(title: str) -> Document:
    doc = Document()
    props = doc.core_properties
    props.author = "letter"
    props.title = title
    return doc


def empty() -> Document:
    return new_document("Пустой документ")


def plain_text() -> Document:
    doc = new_document("Простой текст")
    doc.add_paragraph("Hello, world!")
    doc.add_paragraph(
        "This is a simple paragraph of plain text. It is long enough to wrap "
        "onto several lines when the page is rendered at normal size, which "
        "lets us check line breaking."
    )
    doc.add_paragraph("Третий абзац.")
    return doc


def bold_italic() -> Document:
    doc = new_document("Жирный и курсив")
    p = doc.add_paragraph("Normal, ")
    p.add_run("bold").bold = True
    p.add_run(", ")
    p.add_run("italic").italic = True
    p.add_run(", ")
    r = p.add_run("bold italic")
    r.bold = True
    r.italic = True
    p.add_run(", ")
    p.add_run("underline").underline = True
    p.add_run(", ")
    p.add_run("strike").font.strike = True
    p.add_run(", ")
    r = p.add_run("red 16pt")
    r.font.color.rgb = RGBColor(0xC0, 0x00, 0x00)
    r.font.size = Pt(16)
    p.add_run(", H")
    p.add_run("2").font.subscript = True
    p.add_run("O, x")
    p.add_run("2").font.superscript = True
    p.add_run(".")
    return doc


def headings() -> Document:
    doc = new_document("Заголовки")
    doc.add_heading("Document title", level=0)
    for level in range(1, 4):
        doc.add_heading(f"Heading {level}", level=level)
        doc.add_paragraph(f"Body text under heading level {level}.")
    return doc


def lists() -> Document:
    doc = new_document("Списки")
    doc.add_paragraph("Bulleted list:")
    for item in ("Apples", "Oranges", "Pears"):
        doc.add_paragraph(item, style="List Bullet")
    doc.add_paragraph("Numbered list:")
    for item in ("First", "Second", "Third"):
        doc.add_paragraph(item, style="List Number")
    doc.add_paragraph("Nested item", style="List Bullet 2")
    return doc


def table() -> Document:
    doc = new_document("Таблица")
    doc.add_paragraph("A 3×3 table with a merged cell:")
    t = doc.add_table(rows=3, cols=3)
    t.style = "Table Grid"
    for r in range(3):
        for c in range(3):
            t.cell(r, c).text = f"R{r + 1}C{c + 1}"
    merged = t.cell(2, 0).merge(t.cell(2, 1))
    merged.text = "Merged R3C1–C2"
    doc.add_paragraph("Text after the table.")
    return doc


def image() -> Document:
    doc = new_document("Картинка")
    img = Image.new("RGB", (200, 120), (255, 255, 255))
    draw = ImageDraw.Draw(img)
    draw.rectangle([10, 10, 190, 110], outline=(0, 90, 200), width=4)
    draw.ellipse([60, 25, 140, 95], fill=(230, 120, 0))
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    buf.seek(0)
    doc.add_paragraph("An inline picture below:")
    doc.add_picture(buf, width=Inches(2))
    doc.add_paragraph("Text after the picture.")
    return doc


def header_footer() -> Document:
    doc = new_document("Колонтитулы")
    section = doc.sections[0]
    section.header.paragraphs[0].text = "Header text"
    section.footer.paragraphs[0].text = "Footer text"
    for i in range(1, 4):
        doc.add_paragraph(f"Body paragraph {i}.")
    return doc


def mixed_languages() -> Document:
    doc = new_document("Русский и английский")
    doc.add_paragraph("Съешь же ещё этих мягких французских булок, да выпей чаю.")
    doc.add_paragraph("The quick brown fox jumps over the lazy dog.")
    p = doc.add_paragraph("Смешанный абзац: ")
    p.add_run("mixed paragraph").bold = True
    p.add_run(" — ёлка, «кавычки», № 5, 10 €.")
    return doc


FIXTURES = {
    "empty.docx": empty,
    "plain-text.docx": plain_text,
    "bold-italic.docx": bold_italic,
    "headings.docx": headings,
    "lists.docx": lists,
    "table.docx": table,
    "image.docx": image,
    "header-footer.docx": header_footer,
    "ru-en.docx": mixed_languages,
}


def main() -> None:
    for name, build in FIXTURES.items():
        build().save(OUT / name)
        print(f"wrote {name}")


if __name__ == "__main__":
    main()
