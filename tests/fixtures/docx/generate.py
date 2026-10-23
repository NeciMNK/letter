"""Создаёт тестовые .docx файлы для letter.

Запуск (нужен Python 3 и библиотеки python-docx и Pillow):

    pip install python-docx pillow
    python3 tests/fixtures/docx/generate.py

Файлы появляются рядом с этим скриптом. Содержимое придумано нами,
поэтому файлы можно свободно использовать (лицензия проекта, MIT).
"""

import io
import sys
from pathlib import Path

from docx import Document
from docx.oxml.ns import qn
from docx.shared import Inches, Pt, RGBColor
from lxml import etree
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


def to_anchor(inline, position: str, wrap: str, behind: bool) -> None:
    """Превращает рисунок в строке (`wp:inline`) в плавающий (`wp:anchor`):
    `position` — элементы `wp:positionH` и `wp:positionV`, `wrap` —
    элемент обтекания."""
    from docx.oxml import parse_xml
    from docx.oxml.ns import nsdecls

    extent = inline.find(qn("wp:extent"))
    rest = "".join(
        etree.tostring(child, encoding="unicode")
        for child in inline
        if child.tag not in (qn("wp:extent"), qn("wp:effectExtent"))
    )
    anchor = parse_xml(
        f'<wp:anchor {nsdecls("wp", "a", "pic", "r")} distT="0" distB="0" '
        f'distL="114300" distR="114300" simplePos="0" relativeHeight="1" '
        f'behindDoc="{int(behind)}" locked="0" layoutInCell="1" allowOverlap="1">'
        f'<wp:simplePos x="0" y="0"/>{position}'
        f'<wp:extent cx="{extent.get("cx")}" cy="{extent.get("cy")}"/>'
        f'<wp:effectExtent l="0" t="0" r="0" b="0"/>{wrap}{rest}</wp:anchor>'
    )
    inline.getparent().replace(inline, anchor)


def floating() -> Document:
    """Плавающие рисунки: «вокруг рамки» справа от текста и «за текстом»
    по центру поля."""
    doc = new_document("Плавающие рисунки")
    img = Image.new("RGB", (120, 120), (255, 255, 255))
    draw = ImageDraw.Draw(img)
    draw.ellipse([10, 10, 110, 110], fill=(0, 150, 80))
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    doc.add_paragraph("Floating pictures are anchored to this paragraph.")
    p = doc.add_paragraph("Square wrap: ")
    buf.seek(0)
    run = p.add_run()
    run.add_picture(buf, width=Inches(1))
    to_anchor(
        run._r.find(f".//{qn('wp:inline')}"),
        '<wp:positionH relativeFrom="page"><wp:posOffset>4572000</wp:posOffset>'
        "</wp:positionH>"
        '<wp:positionV relativeFrom="paragraph"><wp:posOffset>127000</wp:posOffset>'
        "</wp:positionV>",
        '<wp:wrapSquare wrapText="bothSides"/>',
        False,
    )
    p.add_run(
        " text after the anchor flows around the picture on its left side"
        " until the lines pass below it." * 4
    )
    p = doc.add_paragraph("Behind text: ")
    buf.seek(0)
    run = p.add_run()
    run.add_picture(buf, width=Inches(0.5))
    to_anchor(
        run._r.find(f".//{qn('wp:inline')}"),
        '<wp:positionH relativeFrom="margin"><wp:align>center</wp:align></wp:positionH>'
        '<wp:positionV relativeFrom="margin"><wp:align>bottom</wp:align></wp:positionV>',
        "<wp:wrapNone/>",
        True,
    )
    p.add_run(" text over the picture.")
    doc.add_paragraph("Text after the pictures.")
    return doc


def set_columns(section, count: int, separator: bool) -> None:
    """Колонки раздела: `w:cols` в его `w:sectPr`."""
    from docx.oxml import OxmlElement

    sect = section._sectPr
    for old in sect.findall(qn("w:cols")):
        sect.remove(old)
    cols = OxmlElement("w:cols")
    cols.set(qn("w:num"), str(count))
    cols.set(qn("w:space"), "708")
    if separator:
        cols.set(qn("w:sep"), "1")
    # w:cols идёт после w:pgMar (порядок CT_SectPr).
    sect.find(qn("w:pgMar")).addnext(cols)


def sections() -> Document:
    """Разделы: на текущей странице с двумя колонками и линией между
    ними, альбомный с новой страницы, с нечётной страницы."""
    from docx.enum.section import WD_ORIENT, WD_SECTION

    doc = new_document("Разделы")
    doc.add_paragraph("First section: one column across the whole page.")
    columns = doc.add_section(WD_SECTION.CONTINUOUS)
    set_columns(columns, 2, True)
    words = (
        "Text in two columns flows down the first column and then continues "
        "at the top of the second one"
    )
    for i in range(1, 7):
        doc.add_paragraph(f"{i}. {words}.")
    after = doc.add_section(WD_SECTION.CONTINUOUS)
    set_columns(after, 1, False)
    doc.add_paragraph("Back to one column below the balanced columns.")
    wide = doc.add_section(WD_SECTION.NEW_PAGE)
    wide.orientation = WD_ORIENT.LANDSCAPE
    wide.page_width, wide.page_height = wide.page_height, wide.page_width
    doc.add_paragraph("A landscape page.")
    odd = doc.add_section(WD_SECTION.ODD_PAGE)
    odd.orientation = WD_ORIENT.PORTRAIT
    odd.page_width, odd.page_height = odd.page_height, odd.page_width
    doc.add_paragraph("This section starts on an odd page.")
    return doc


def header_footer() -> Document:
    doc = new_document("Колонтитулы")
    section = doc.sections[0]
    section.header.paragraphs[0].text = "Header text"
    section.footer.paragraphs[0].text = "Footer text"
    for i in range(1, 4):
        doc.add_paragraph(f"Body paragraph {i}.")
    return doc


def add_field(paragraph, code: str, result: str) -> None:
    """Поле Word из знаков поля: начало, код, разделитель, результат, конец."""
    from docx.oxml import OxmlElement

    def char(kind: str) -> None:
        run = paragraph.add_run()
        element = OxmlElement("w:fldChar")
        element.set(qn("w:fldCharType"), kind)
        run._r.append(element)

    char("begin")
    instruction = OxmlElement("w:instrText")
    instruction.set("{http://www.w3.org/XML/1998/namespace}space", "preserve")
    instruction.text = code
    paragraph.add_run()._r.append(instruction)
    char("separate")
    paragraph.add_run(result)
    char("end")


def add_hyperlink(paragraph, text: str, url: str | None = None, anchor: str | None = None) -> None:
    """Гиперссылка со стилем «Hyperlink»: на адрес (через связь) или на
    закладку."""
    from docx.opc.constants import RELATIONSHIP_TYPE
    from docx.oxml import OxmlElement

    link = OxmlElement("w:hyperlink")
    if url is not None:
        rid = paragraph.part.relate_to(url, RELATIONSHIP_TYPE.HYPERLINK, is_external=True)
        link.set(qn("r:id"), rid)
    if anchor is not None:
        link.set(qn("w:anchor"), anchor)
    link.set(qn("w:history"), "1")
    run = OxmlElement("w:r")
    props = OxmlElement("w:rPr")
    style = OxmlElement("w:rStyle")
    style.set(qn("w:val"), "Hyperlink")
    props.append(style)
    run.append(props)
    text_element = OxmlElement("w:t")
    text_element.text = text
    run.append(text_element)
    link.append(run)
    paragraph._p.append(link)


def add_bookmark(paragraph, name: str, text: str, number: int) -> None:
    """Закладка вокруг текста `text`."""
    from docx.oxml import OxmlElement

    start = OxmlElement("w:bookmarkStart")
    start.set(qn("w:id"), str(number))
    start.set(qn("w:name"), name)
    paragraph._p.append(start)
    paragraph.add_run(text)
    end = OxmlElement("w:bookmarkEnd")
    end.set(qn("w:id"), str(number))
    paragraph._p.append(end)


def links() -> Document:
    """Гиперссылки на сайт и на закладку, закладка вокруг слова."""
    doc = new_document("Ссылки")
    p = doc.add_paragraph("Site: ")
    add_hyperlink(p, "example site", url="https://example.com/page?a=1&b=2")
    p.add_run(".")
    p = doc.add_paragraph("Go to ")
    add_hyperlink(p, "the target", anchor="target")
    p.add_run(" below.")
    p = doc.add_paragraph("This is ")
    add_bookmark(p, "target", "the target", 0)
    p.add_run(" paragraph.")
    return doc


def add_notes_part(doc, kind: str, notes: list[tuple[int, str]]) -> None:
    """Часть сносок (`footnotes`) или концевых сносок (`endnotes`) со
    служебными сносками-чертами и текстами `notes`."""
    from docx.opc.constants import RELATIONSHIP_TYPE
    from docx.opc.packuri import PackURI
    from docx.opc.part import Part

    w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
    tag = kind[:-1]
    mark = "footnoteRef" if kind == "footnotes" else "endnoteRef"
    text_style = "FootnoteText" if kind == "footnotes" else "EndnoteText"
    ref_style = "FootnoteReference" if kind == "footnotes" else "EndnoteReference"
    r = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
    xml = f'<w:{kind} xmlns:w="{w}" xmlns:r="{r}">'
    for special, number in (("separator", -1), ("continuationSeparator", 0)):
        xml += (
            f'<w:{tag} w:type="{special}" w:id="{number}"><w:p><w:pPr>'
            '<w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr>'
            f"<w:r><w:{special}/></w:r></w:p></w:{tag}>"
        )
    for number, text in notes:
        xml += (
            f'<w:{tag} w:id="{number}"><w:p><w:pPr><w:pStyle w:val="{text_style}"/></w:pPr>'
            f'<w:r><w:rPr><w:rStyle w:val="{ref_style}"/></w:rPr><w:{mark}/></w:r>'
            f'<w:r><w:t xml:space="preserve"> {text}</w:t></w:r></w:p></w:{tag}>'
        )
    xml += f"</w:{kind}>"
    content_type = (
        f"application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml"
    )
    part = Part(PackURI(f"/word/{kind}.xml"), content_type, xml.encode(), doc.part.package)
    relationship = (
        RELATIONSHIP_TYPE.FOOTNOTES if kind == "footnotes" else RELATIONSHIP_TYPE.ENDNOTES
    )
    doc.part.relate_to(part, relationship)


def add_note_reference(paragraph, kind: str, number: int) -> None:
    """Ссылка на сноску (`kind` — `footnote` или `endnote`) со стилем
    «Знак сноски»."""
    from docx.oxml import OxmlElement

    run = paragraph.add_run()
    run.style = "Footnote Reference" if kind == "footnote" else "Endnote Reference"
    reference = OxmlElement(f"w:{kind}Reference")
    reference.set(qn("w:id"), str(number))
    run._r.append(reference)


def notes() -> Document:
    """Две сноски и концевая сноска."""
    from docx.enum.style import WD_STYLE_TYPE

    doc = new_document("Сноски")
    for name in ("Footnote Reference", "Endnote Reference"):
        style = doc.styles.add_style(name, WD_STYLE_TYPE.CHARACTER)
        style.font.superscript = True
    for name in ("Footnote Text", "Endnote Text"):
        style = doc.styles.add_style(name, WD_STYLE_TYPE.PARAGRAPH)
        style.font.size = Pt(10)
    p = doc.add_paragraph("First claim")
    add_note_reference(p, "footnote", 1)
    p.add_run(" and second claim")
    add_note_reference(p, "footnote", 2)
    p.add_run(".")
    p = doc.add_paragraph("A remark at the end")
    add_note_reference(p, "endnote", 1)
    p.add_run(".")
    add_notes_part(doc, "footnotes", [(1, "First footnote."), (2, "Second footnote.")])
    add_notes_part(doc, "endnotes", [(1, "The endnote.")])
    return doc


def add_comment_parts(doc, comments: list[tuple[int, str, str, str, int | None, bool]]) -> None:
    """Части примечаний (`comments.xml`) и ответов с отметкой «решено»
    (`commentsExtended.xml`), как их пишет Word: номер, автор, инициалы,
    текст, на какое примечание ответ, решено ли."""
    from docx.opc.packuri import PackURI
    from docx.opc.part import Part

    w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
    w14 = "http://schemas.microsoft.com/office/word/2010/wordml"
    w15 = "http://schemas.microsoft.com/office/word/2012/wordml"
    mc = "http://schemas.openxmlformats.org/markup-compatibility/2006"
    r = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
    xml = (
        f'<w:comments xmlns:w="{w}" xmlns:r="{r}" xmlns:w14="{w14}" xmlns:mc="{mc}" '
        'mc:Ignorable="w14">'
    )
    extended = f'<w15:commentsEx xmlns:w15="{w15}">'
    for number, author, initials, text, parent, done in comments:
        para = f"{0x1000 + number:08X}"
        xml += (
            f'<w:comment w:id="{number}" w:author="{author}" '
            f'w:date="2026-10-23T10:0{number}:00Z" w:initials="{initials}">'
            f'<w:p w14:paraId="{para}"><w:pPr><w:pStyle w:val="CommentText"/></w:pPr>'
            '<w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:annotationRef/></w:r>'
            f"<w:r><w:t>{text}</w:t></w:r></w:p></w:comment>"
        )
        parent_attr = "" if parent is None else f' w15:paraIdParent="{0x1000 + parent:08X}"'
        extended += f'<w15:commentEx w15:paraId="{para}"{parent_attr} w15:done="{int(done)}"/>'
    xml += "</w:comments>"
    extended += "</w15:commentsEx>"
    base = "application/vnd.openxmlformats-officedocument.wordprocessingml"
    for name, content, content_type, relationship in (
        (
            "comments",
            xml,
            f"{base}.comments+xml",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments",
        ),
        (
            "commentsExtended",
            extended,
            f"{base}.commentsExtended+xml",
            "http://schemas.microsoft.com/office/2011/relationships/commentsExtended",
        ),
    ):
        part = Part(
            PackURI(f"/word/{name}.xml"), content_type, content.encode(), doc.part.package
        )
        doc.part.relate_to(part, relationship)


def comment_range(paragraph, ids: list[int], before: str, text: str, after: str) -> None:
    """Текст `text` с примечаниями `ids` (начало, конец и ссылка, как
    у Word), между `before` и `after`."""
    from docx.oxml import OxmlElement

    def mark(tag: str, number: int):
        element = OxmlElement(f"w:{tag}")
        element.set(qn("w:id"), str(number))
        return element

    paragraph.add_run(before)
    for number in ids:
        paragraph._p.append(mark("commentRangeStart", number))
    paragraph.add_run(text)
    for number in ids:
        paragraph._p.append(mark("commentRangeEnd", number))
        run = paragraph.add_run()
        run.style = "Comment Reference"
        run._r.append(mark("commentReference", number))
    paragraph.add_run(after)


def comments() -> Document:
    """Два примечания; на первое есть ответ, второе отмечено «решено»."""
    from docx.enum.style import WD_STYLE_TYPE

    doc = new_document("Примечания")
    style = doc.styles.add_style("Comment Reference", WD_STYLE_TYPE.CHARACTER)
    style.font.size = Pt(8)
    style = doc.styles.add_style("Comment Text", WD_STYLE_TYPE.PARAGRAPH)
    style.font.size = Pt(10)
    comment_range(doc.add_paragraph(), [0, 1], "The ", "quick brown", " fox.")
    comment_range(doc.add_paragraph(), [2], "Jumps over ", "the lazy dog", ".")
    add_comment_parts(
        doc,
        [
            (0, "Anna Reviewer", "AR", "Which fox?", None, False),
            (1, "Boris Author", "BA", "The usual one.", 0, False),
            (2, "Anna Reviewer", "AR", "Fixed already.", None, True),
        ],
    )
    return doc


def headers() -> Document:
    """Колонтитулы: особый для первой страницы, разные для чётных
    и нечётных; второй раздел берёт верхний колонтитул первого, а нижний
    у него свой."""
    from docx.enum.section import WD_SECTION

    doc = new_document("Колонтитулы")
    doc.settings.odd_and_even_pages_header_footer = True
    first = doc.sections[0]
    first.different_first_page_header_footer = True
    first.first_page_header.paragraphs[0].text = "First page header"
    first.header.paragraphs[0].text = "Odd page header"
    first.even_page_header.paragraphs[0].text = "Even page header"
    odd = first.footer.paragraphs[0]
    odd.text = "Odd page footer, page "
    add_field(odd, " PAGE ", "1")
    odd.add_run(" of ")
    add_field(odd, " NUMPAGES ", "1")
    first.even_page_footer.paragraphs[0].text = "Even page footer"
    first.first_page_footer.paragraphs[0].text = "First page footer"
    text = (
        "Body text that fills the pages so that the headers and footers of "
        "the first, even and odd pages can be seen"
    )
    for i in range(1, 91):
        doc.add_paragraph(f"{i}. {text}.")
    second = doc.add_section(WD_SECTION.NEW_PAGE)
    second.different_first_page_header_footer = False
    second.footer.is_linked_to_previous = False
    second.footer.paragraphs[0].text = "Second section footer"
    second.even_page_footer.is_linked_to_previous = False
    second.even_page_footer.paragraphs[0].text = "Second section even footer"
    doc.add_paragraph("The second section keeps the header of the first one.")
    return doc


def mixed_languages() -> Document:
    doc = new_document("Русский и английский")
    doc.add_paragraph("Съешь же ещё этих мягких французских булок, да выпей чаю.")
    doc.add_paragraph("The quick brown fox jumps over the lazy dog.")
    p = doc.add_paragraph("Смешанный абзац: ")
    p.add_run("mixed paragraph").bold = True
    p.add_run(" — ёлка, «кавычки», № 5, 10 €.")
    return doc


def pages() -> Document:
    """Несколько страниц: переносы строк, разрыв страницы, заголовки
    («не отрывать от следующего») и «не разрывать абзац» — для сверки
    вёрстки с Word."""
    doc = new_document("Несколько страниц")
    words = (
        "letter lays out pages the same way Word does: lines, paragraphs, "
        "spacing and page breaks must land in the same places"
    ).split()
    for chapter in range(1, 4):
        doc.add_heading(f"Chapter {chapter}", level=1)
        for i in range(1, 9):
            count = 12 + (chapter * 7 + i * 5) % 40
            text = " ".join(words[(i + k) % len(words)] for k in range(count))
            p = doc.add_paragraph(f"{chapter}.{i}. {text.capitalize()}.")
            if i == 5:
                p.paragraph_format.keep_together = True
        if chapter == 1:
            doc.add_page_break()
    return doc


def set_borders(paragraph, sides: dict[str, tuple[str, int, int, str]], fill: str | None = None) -> None:
    """Границы абзаца (`w:pBdr`): сторона → (вид, толщина в 1/8 пт,
    отступ в пт, цвет); `fill` — цвет заливки (`w:shd`)."""
    ppr = paragraph._p.get_or_add_pPr()
    pbdr = etree.SubElement(ppr, qn("w:pBdr")) if sides else None
    for side, (style, size, space, color) in sides.items():
        border = etree.SubElement(pbdr, qn(f"w:{side}"))
        border.set(qn("w:val"), style)
        border.set(qn("w:sz"), str(size))
        border.set(qn("w:space"), str(space))
        border.set(qn("w:color"), color)
    if fill:
        shd = etree.SubElement(ppr, qn("w:shd"))
        shd.set(qn("w:val"), "clear")
        shd.set(qn("w:color"), "auto")
        shd.set(qn("w:fill"), fill)


def borders() -> Document:
    """Границы и заливка абзацев: рамка вокруг двух абзацев с линией
    между ними, линия под заголовком, заливка без линий; цвет страницы
    и рамка страницы."""
    doc = new_document("Границы и заливка")
    box = {
        side: ("single", 8, 4 if side in ("left", "right") else 1, "1F4E79")
        for side in ("top", "left", "bottom", "right", "between")
    }
    for text in ("Первый абзац в рамке.", "Второй абзац в той же рамке."):
        set_borders(doc.add_paragraph(text), box, fill="DEEAF6")
    set_borders(doc.add_paragraph("Заголовок с линией снизу"), {"bottom": ("double", 6, 1, "auto")})
    set_borders(doc.add_paragraph("Абзац с заливкой без линий."), {}, fill="FFF2CC")
    doc.add_paragraph("Обычный абзац.")
    # Цвет страницы (с флагом в настройках, без которого Word его не
    # показывает) и рамка страницы в 24 пт от края листа.
    background = etree.Element(qn("w:background"))
    background.set(qn("w:color"), "EAF1DD")
    doc.element.insert(0, background)
    zoom = doc.settings.element.find(qn("w:zoom"))
    flag = etree.Element(qn("w:displayBackgroundShape"))
    if zoom is not None:
        zoom.addnext(flag)
    else:
        doc.settings.element.insert(0, flag)
    sect = doc.sections[0]._sectPr
    page_borders = etree.Element(qn("w:pgBorders"))
    page_borders.set(qn("w:offsetFrom"), "page")
    for side in ("top", "left", "bottom", "right"):
        border = etree.SubElement(page_borders, qn(f"w:{side}"))
        border.set(qn("w:val"), "single")
        border.set(qn("w:sz"), "12")
        border.set(qn("w:space"), "24")
        border.set(qn("w:color"), "auto")
    sect.find(qn("w:pgMar")).addnext(page_borders)
    return doc


WATERMARK = (
    '<w:pict xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
    'xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office">'
    '<v:shapetype id="_x0000_t136" coordsize="21600,21600" o:spt="136" adj="10800" '
    'path="m@7,l@8,m@5,21600l@6,21600e"><v:path textpathok="t" o:connecttype="custom"/>'
    '<v:textpath on="t" fitshape="t"/></v:shapetype>'
    '<v:shape id="PowerPlusWaterMarkObject1" o:spid="_x0000_s2049" type="#_x0000_t136" '
    'style="position:absolute;margin-left:0;margin-top:0;width:412.4pt;height:137.45pt;'
    "rotation:315;z-index:-251657216;mso-position-horizontal:center;"
    "mso-position-horizontal-relative:margin;mso-position-vertical:center;"
    'mso-position-vertical-relative:margin" o:allowincell="f" fillcolor="silver" stroked="f">'
    '<v:fill opacity=".5"/><v:textpath style="font-family:&quot;Calibri&quot;;font-size:1pt" '
    'string="ОБРАЗЕЦ"/></v:shape></w:pict>'
)


def watermark() -> Document:
    """Подложка «ОБРАЗЕЦ», как её вставляет Word: фигура VML с текстом
    в верхнем колонтитуле."""
    doc = new_document("Подложка")
    for i in range(3):
        doc.add_paragraph(f"Абзац {i + 1} документа с подложкой.")
    header = doc.sections[0].header
    run = header.paragraphs[0].add_run()
    run._r.append(etree.fromstring(WATERMARK))
    return doc


C_NS = "http://schemas.openxmlformats.org/drawingml/2006/chart"
A_NS = "http://schemas.openxmlformats.org/drawingml/2006/main"
R_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"


def chart_series(index: int, name: str, categories: list[str], values: list[float], color: str | None) -> str:
    """Ряд `c:ser` с кешем имени, подписей и значений (как пишет Word)."""
    cats = "".join(f'<c:pt idx="{i}"><c:v>{c}</c:v></c:pt>' for i, c in enumerate(categories))
    vals = "".join(f'<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>' for i, v in enumerate(values))
    column = "BCDEFG"[index]
    fill = (
        f'<c:spPr><a:solidFill><a:schemeClr val="{color}"/></a:solidFill></c:spPr>'
        if color
        else ""
    )
    return (
        f'<c:ser><c:idx val="{index}"/><c:order val="{index}"/>'
        f'<c:tx><c:strRef><c:f>Лист1!${column}$1</c:f><c:strCache><c:ptCount val="1"/>'
        f'<c:pt idx="0"><c:v>{name}</c:v></c:pt></c:strCache></c:strRef></c:tx>{fill}'
        f'<c:cat><c:strRef><c:f>Лист1!$A$2:$A${len(categories) + 1}</c:f><c:strCache>'
        f'<c:ptCount val="{len(categories)}"/>{cats}</c:strCache></c:strRef></c:cat>'
        f'<c:val><c:numRef><c:f>Лист1!${column}$2:${column}${len(values) + 1}</c:f><c:numCache>'
        f'<c:formatCode>General</c:formatCode><c:ptCount val="{len(values)}"/>{vals}'
        "</c:numCache></c:numRef></c:val></c:ser>"
    )


def add_chart(doc, paragraph, number: int, plot: str, title: str | None, legend: str | None) -> None:
    """Часть диаграммы `word/charts/chartN.xml` (только кеш, без таблицы
    с данными) и рисунок-диаграмма в строке абзаца."""
    from docx.opc.constants import RELATIONSHIP_TYPE
    from docx.opc.packuri import PackURI
    from docx.opc.part import Part

    title_xml = (
        f'<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>{title}</a:t></a:r></a:p>'
        '</c:rich></c:tx><c:overlay val="0"/></c:title><c:autoTitleDeleted val="0"/>'
        if title
        else '<c:autoTitleDeleted val="1"/>'
    )
    legend_xml = (
        f'<c:legend><c:legendPos val="{legend}"/><c:overlay val="0"/></c:legend>' if legend else ""
    )
    xml = (
        f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        f'<c:chartSpace xmlns:c="{C_NS}" xmlns:a="{A_NS}" xmlns:r="{R_NS}">'
        f'<c:roundedCorners val="0"/><c:chart>{title_xml}<c:plotArea><c:layout/>{plot}</c:plotArea>'
        f'{legend_xml}<c:plotVisOnly val="1"/></c:chart></c:chartSpace>'
    )
    part = Part(
        PackURI(f"/word/charts/chart{number}.xml"),
        "application/vnd.openxmlformats-officedocument.drawingml.chart+xml",
        xml.encode(),
        doc.part.package,
    )
    rid = doc.part.relate_to(part, RELATIONSHIP_TYPE.CHART)
    cx, cy = Inches(5).emu, Inches(3).emu
    drawing = (
        f'<w:drawing xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
        '<wp:inline distT="0" distB="0" distL="0" distR="0" '
        'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing">'
        f'<wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>'
        f'<wp:docPr id="{number}" name="Диаграмма {number}"/><wp:cNvGraphicFramePr/>'
        f'<a:graphic xmlns:a="{A_NS}"><a:graphicData uri="{C_NS}">'
        f'<c:chart xmlns:c="{C_NS}" xmlns:r="{R_NS}" r:id="{rid}"/>'
        "</a:graphicData></a:graphic></wp:inline></w:drawing>"
    )
    run = paragraph.add_run()
    run._r.append(etree.fromstring(drawing))


def charts() -> Document:
    doc = new_document("Диаграммы")
    months = ["Январь", "Февраль", "Март", "Апрель"]
    doc.add_paragraph("Гистограмма с группировкой, два ряда:")
    columns = (
        '<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:varyColors val="0"/>'
        + chart_series(0, "Москва", months, [4.3, 2.5, 3.5, 4.5], "accent1")
        + chart_series(1, "Казань", months, [2.4, 4.4, 1.8, 2.8], "accent2")
        + '<c:gapWidth val="219"/><c:overlap val="-27"/><c:axId val="1"/><c:axId val="2"/></c:barChart>'
        '<c:catAx><c:axId val="1"/><c:axPos val="b"/><c:crossAx val="2"/></c:catAx>'
        '<c:valAx><c:axId val="2"/><c:majorGridlines/><c:axPos val="l"/><c:crossAx val="1"/></c:valAx>'
    )
    add_chart(doc, doc.add_paragraph(), 1, columns, "Продажи", "b")
    doc.add_paragraph("Круговая диаграмма:")
    pie = (
        '<c:pieChart><c:varyColors val="1"/>'
        + chart_series(0, "Доля", ["Чай", "Кофе", "Сок"], [50, 30, 20], None)
        + '<c:firstSliceAng val="0"/></c:pieChart>'
    )
    add_chart(doc, doc.add_paragraph(), 2, pie, None, "r")
    doc.add_paragraph("График:")
    line = (
        '<c:lineChart><c:grouping val="standard"/><c:varyColors val="0"/>'
        + chart_series(0, "Температура", months, [-8, -6, 1, 9], None)
        + '<c:marker val="1"/><c:axId val="3"/><c:axId val="4"/></c:lineChart>'
        '<c:catAx><c:axId val="3"/><c:axPos val="b"/><c:crossAx val="4"/></c:catAx>'
        '<c:valAx><c:axId val="4"/><c:majorGridlines/><c:axPos val="l"/><c:crossAx val="3"/></c:valAx>'
    )
    add_chart(doc, doc.add_paragraph(), 3, line, None, None)
    doc.add_paragraph("Текст после диаграмм.")
    return doc


FIXTURES = {
    "empty.docx": empty,
    "plain-text.docx": plain_text,
    "bold-italic.docx": bold_italic,
    "headings.docx": headings,
    "lists.docx": lists,
    "table.docx": table,
    "image.docx": image,
    "floating.docx": floating,
    "sections.docx": sections,
    "header-footer.docx": header_footer,
    "ru-en.docx": mixed_languages,
    "pages.docx": pages,
    "headers.docx": headers,
    "links.docx": links,
    "notes.docx": notes,
    "comments.docx": comments,
    "borders.docx": borders,
    "watermark.docx": watermark,
    "charts.docx": charts,
}


def main() -> None:
    # Можно пересоздать только некоторые файлы: generate.py pages.docx
    names = sys.argv[1:] or list(FIXTURES)
    for name in names:
        build = FIXTURES[name]
        build().save(OUT / name)
        print(f"wrote {name}")


if __name__ == "__main__":
    main()
