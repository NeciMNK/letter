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

DGM_NS = "http://schemas.openxmlformats.org/drawingml/2006/diagram"
DSP_NS = "http://schemas.microsoft.com/office/drawing/2008/diagram"
A_NS = "http://schemas.openxmlformats.org/drawingml/2006/main"
R_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
DGM_REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/"
DGM_TYPE = "application/vnd.openxmlformats-officedocument.drawingml."


def smartart_shape(number: int, prst: str, x: int, y: int, cx: int, cy: int, fill: str, text: str | None) -> str:
    """Фигура готового вида SmartArt, как её пишет Word: цвет из темы,
    стиль со ссылками на заливку, линию и шрифт."""
    body = ""
    if text is not None:
        body = (
            '<dsp:txBody><a:bodyPr spcFirstLastPara="0" vert="horz" wrap="square" '
            'lIns="76200" tIns="76200" rIns="76200" bIns="76200" numCol="1" '
            'spcCol="1270" anchor="ctr" anchorCtr="0"><a:noAutofit/></a:bodyPr>'
            '<a:lstStyle/><a:p><a:pPr marL="0" lvl="0" indent="0" algn="ctr" '
            'defTabSz="889000"><a:lnSpc><a:spcPct val="90000"/></a:lnSpc></a:pPr>'
            f'<a:r><a:rPr lang="ru-RU" sz="2000" kern="1200"/><a:t>{text}</a:t></a:r></a:p>'
            f'</dsp:txBody><dsp:txXfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></dsp:txXfrm>'
        )
    return (
        f'<dsp:sp modelId="{{00000000-0000-0000-0000-00000000000{number}}}">'
        f'<dsp:nvSpPr><dsp:cNvPr id="0" name=""/><dsp:cNvSpPr/></dsp:nvSpPr>'
        f'<dsp:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>'
        f'<a:prstGeom prst="{prst}"><a:avLst/></a:prstGeom>{fill}'
        '<a:ln w="12700" cap="flat" cmpd="sng" algn="ctr"><a:solidFill>'
        '<a:schemeClr val="lt1"><a:hueOff val="0"/></a:schemeClr></a:solidFill>'
        '<a:prstDash val="solid"/><a:miter lim="800000"/></a:ln><a:effectLst/></dsp:spPr>'
        '<dsp:style><a:lnRef idx="2"><a:scrgbClr r="0" g="0" b="0"/></a:lnRef>'
        '<a:fillRef idx="1"><a:scrgbClr r="0" g="0" b="0"/></a:fillRef>'
        '<a:effectRef idx="0"><a:scrgbClr r="0" g="0" b="0"/></a:effectRef>'
        '<a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></dsp:style>'
        f"{body}</dsp:sp>"
    )


def smartart() -> Document:
    """SmartArt «Простой процесс»: три скруглённых прямоугольника со
    стрелками между ними, с готовым видом (drawing1.xml), как у Word."""
    from docx.opc.constants import RELATIONSHIP_TYPE as RT  # noqa: F401
    from docx.opc.packuri import PackURI
    from docx.opc.part import Part

    doc = new_document("SmartArt")
    doc.add_paragraph("Перед диаграммой.")
    main = doc.part
    package = main.package

    box = '<a:solidFill><a:schemeClr val="accent1"><a:hueOff val="0"/><a:lumMod val="100000"/></a:schemeClr></a:solidFill>'
    arrow = '<a:solidFill><a:schemeClr val="accent1"><a:tint val="60000"/></a:schemeClr></a:solidFill>'
    shapes = [
        smartart_shape(1, "roundRect", 0, 388620, 1371600, 822960, box, "Идея"),
        smartart_shape(2, "rightArrow", 1508760, 629055, 290830, 340090, arrow, None),
        smartart_shape(3, "roundRect", 2057400, 388620, 1371600, 822960, box, "Черновик"),
        smartart_shape(4, "rightArrow", 3566160, 629055, 290830, 340090, arrow, None),
        smartart_shape(5, "roundRect", 4114800, 388620, 1371600, 822960, box, "Книга"),
    ]
    drawing_xml = (
        f'<dsp:drawing xmlns:dgm="{DGM_NS}" xmlns:dsp="{DSP_NS}" xmlns:a="{A_NS}">'
        '<dsp:spTree><dsp:nvGrpSpPr><dsp:cNvPr id="0" name=""/><dsp:cNvGrpSpPr/></dsp:nvGrpSpPr>'
        f'<dsp:grpSpPr/>{"".join(shapes)}</dsp:spTree></dsp:drawing>'
    )

    def add(name: str, content_type: str, reltype: str, xml: str) -> str:
        part = Part(PackURI(name), content_type, xml.encode("utf-8"), package)
        return main.relate_to(part, reltype)

    drawing_id = add(
        "/word/diagrams/drawing1.xml",
        "application/vnd.ms-office.drawingml.diagramDrawing+xml",
        "http://schemas.microsoft.com/office/2007/relationships/diagramDrawing",
        drawing_xml,
    )
    points = "".join(
        f'<dgm:pt modelId="{{10000000-0000-0000-0000-00000000000{i}}}"><dgm:prSet phldrT="[Текст]"/>'
        f'<dgm:spPr/><dgm:t><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="ru-RU"/><a:t>{t}</a:t></a:r></a:p></dgm:t></dgm:pt>'
        for i, t in enumerate(["Идея", "Черновик", "Книга"], start=1)
    )
    data_xml = (
        f'<dgm:dataModel xmlns:dgm="{DGM_NS}" xmlns:a="{A_NS}" xmlns:r="{R_NS}">'
        '<dgm:ptLst><dgm:pt modelId="{10000000-0000-0000-0000-000000000000}" type="doc">'
        '<dgm:prSet loTypeId="urn:microsoft.com/office/officeart/2005/8/layout/process1"/>'
        f'<dgm:spPr/><dgm:t><a:bodyPr/><a:lstStyle/><a:p/></dgm:t></dgm:pt>{points}</dgm:ptLst>'
        '<dgm:cxnLst/><dgm:bg/><dgm:whole/><dgm:extLst>'
        '<a:ext uri="http://schemas.microsoft.com/office/drawing/2008/diagram">'
        f'<dsp:dataModelExt xmlns:dsp="{DSP_NS}" relId="{drawing_id}" minVer="http://schemas.openxmlformats.org/drawingml/2006/diagram"/>'
        "</a:ext></dgm:extLst></dgm:dataModel>"
    )
    dm = add("/word/diagrams/data1.xml", DGM_TYPE + "diagramData+xml", DGM_REL + "diagramData", data_xml)
    lo = add(
        "/word/diagrams/layout1.xml",
        DGM_TYPE + "diagramLayout+xml",
        DGM_REL + "diagramLayout",
        f'<dgm:layoutDef xmlns:dgm="{DGM_NS}" xmlns:a="{A_NS}" '
        'uniqueId="urn:microsoft.com/office/officeart/2005/8/layout/process1">'
        '<dgm:layoutNode name="Name0"/></dgm:layoutDef>',
    )
    qs = add(
        "/word/diagrams/quickStyle1.xml",
        DGM_TYPE + "diagramStyle+xml",
        DGM_REL + "diagramQuickStyle",
        f'<dgm:styleDef xmlns:dgm="{DGM_NS}" xmlns:a="{A_NS}" '
        'uniqueId="urn:microsoft.com/office/officeart/2005/8/quickstyle/simple1"><dgm:scene3d>'
        '<a:camera prst="orthographicFront"/><a:lightRig rig="threePt" dir="t"/></dgm:scene3d></dgm:styleDef>',
    )
    cs = add(
        "/word/diagrams/colors1.xml",
        DGM_TYPE + "diagramColors+xml",
        DGM_REL + "diagramColors",
        f'<dgm:colorsDef xmlns:dgm="{DGM_NS}" xmlns:a="{A_NS}" '
        'uniqueId="urn:microsoft.com/office/officeart/2005/8/colors/accent1_2"/>',
    )
    inline = (
        f'<w:drawing xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
        f'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
        f'xmlns:a="{A_NS}" xmlns:r="{R_NS}" xmlns:dgm="{DGM_NS}">'
        '<wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="5486400" cy="1600200"/>'
        '<wp:effectExtent l="0" t="0" r="0" b="0"/><wp:docPr id="1" name="Схема 1"/>'
        '<wp:cNvGraphicFramePr/><a:graphic><a:graphicData '
        'uri="http://schemas.openxmlformats.org/drawingml/2006/diagram">'
        f'<dgm:relIds r:dm="{dm}" r:lo="{lo}" r:qs="{qs}" r:cs="{cs}"/>'
        "</a:graphicData></a:graphic></wp:inline></w:drawing>"
    )
    run = doc.add_paragraph().add_run()
    run._r.append(etree.fromstring(inline))
    doc.add_paragraph("После диаграммы.")
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
    "borders.docx": borders,
    "watermark.docx": watermark,
    "smartart.docx": smartart,
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
