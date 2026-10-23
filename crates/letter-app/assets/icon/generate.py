# Рисует значок letter: лист с загнутым уголком и строками текста на
# синем скруглённом квадрате. Свой рисунок, не значок Microsoft.
import sys
from PIL import Image, ImageDraw
out = sys.argv[1]
S = 1024
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
# Фон: скруглённый квадрат, вертикальный градиент.
bg = Image.new("RGBA", (S, S))
top, bottom = (79, 109, 245), (45, 64, 186)
for y in range(S):
    t = y / (S - 1)
    c = tuple(round(top[i] + (bottom[i] - top[i]) * t) for i in range(3)) + (255,)
    ImageDraw.Draw(bg).line([(0, y), (S, y)], fill=c)
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle([40, 40, S - 40, S - 40], radius=200, fill=255)
img.paste(bg, (0, 0), mask)
# Лист бумаги с загнутым уголком.
x0, y0, x1, y1, fold = 260, 190, 764, 834, 150
d.polygon([(x0, y0), (x1 - fold, y0), (x1, y0 + fold), (x1, y1), (x0, y1)], fill=(255, 255, 255, 255))
d.polygon([(x1 - fold, y0), (x1 - fold, y0 + fold), (x1, y0 + fold)], fill=(196, 206, 250, 255))
# Строки текста; первая — заголовок, последняя короче.
lines = [(330, 300, 560, 40, (45, 64, 186)), (330, 420, 694, 26, (150, 160, 190)),
         (330, 490, 694, 26, (150, 160, 190)), (330, 560, 694, 26, (150, 160, 190)),
         (330, 630, 694, 26, (150, 160, 190)), (330, 700, 560, 26, (150, 160, 190))]
for (a, y, b, h, c) in lines:
    d.rounded_rectangle([a, y, b, y + h], radius=h // 2, fill=c + (255,))
img.save(f"{out}/letter.png")
for n in (16, 32, 48, 64, 128, 256, 512):
    img.resize((n, n), Image.LANCZOS).save(f"{out}/letter-{n}.png")
img.save(f"{out}/letter.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
img.save(f"{out}/letter.icns")
