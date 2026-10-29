"""Добавляет в шрифты Onest недостающие символы из DejaVu / FreeSans / DejaVu Math.
Цветные эмодзи (Emoji_Presentation) не трогаем: их рисует системный шрифт эмодзи."""
import sys, re
from fontTools.ttLib import TTFont
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.recordingPen import DecomposingRecordingPen

EMOJI_DATA = "/usr/share/texlive/texmf-dist/tex/generic/unicode-data/emoji-data.txt"
FONTS = "/usr/share/fonts/truetype"

def emoji_presentation():
    s = set()
    for ln in open(EMOJI_DATA, encoding="utf-8"):
        ln = ln.split("#")[0].strip()
        if not ln or "Emoji_Presentation" not in ln:
            continue
        cp = ln.split(";")[0].strip()
        if ".." in cp:
            a, b = cp.split("..")
            s.update(range(int(a, 16), int(b, 16) + 1))
        else:
            s.add(int(cp, 16))
    return s

RANGES = [
    (0x0250, 0x02AF), (0x02B0, 0x02FF), (0x1D00, 0x1DBF), (0x2000, 0x206F), (0x2070, 0x209F),
    (0x20A0, 0x20CF), (0x2100, 0x214F), (0x2150, 0x218F), (0x2190, 0x21FF), (0x2200, 0x22FF),
    (0x2300, 0x23FF), (0x2400, 0x243F), (0x2460, 0x24FF), (0x2500, 0x257F), (0x2580, 0x259F),
    (0x25A0, 0x25FF), (0x2600, 0x26FF), (0x2700, 0x27BF), (0x27C0, 0x27FF), (0x2800, 0x28FF),
    (0x2900, 0x297F), (0x2980, 0x29FF), (0x2A00, 0x2AFF), (0x2B00, 0x2BFF), (0x2E00, 0x2E7F),
    (0x3200, 0x32FF), (0xA700, 0xA71F), (0x1D400, 0x1D7FF), (0x1F100, 0x1F1FF), (0x1F650, 0x1F67F),
]

def donors(bold):
    d = "DejaVuSans-Bold.ttf" if bold else "DejaVuSans.ttf"
    f = "FreeSansBold.ttf" if bold else "FreeSans.ttf"
    return [
        f"{FONTS}/dejavu/{d}",
        f"{FONTS}/freefont/{f}",
        f"{FONTS}/freefont/FreeSans.ttf",
        f"{FONTS}/dejavu/DejaVuMathTeXGyre.ttf",
        f"{FONTS}/freefont/FreeSerif.ttf",
    ]

def merge(path, bold):
    skip = emoji_presentation()
    base = TTFont(path)
    cmap = base.getBestCmap()
    upm = base["head"].unitsPerEm
    glyf = base["glyf"]
    hmtx = base["hmtx"]
    order = base.getGlyphOrder()
    added = 0
    want = [cp for a, b in RANGES for cp in range(a, b + 1) if cp not in cmap and cp not in skip]
    for dpath in donors(bold):
        try:
            donor = TTFont(dpath)
        except Exception as e:
            print("skip donor", dpath, e)
            continue
        if "glyf" not in donor:
            print("skip non-glyf", dpath)
            continue
        dcmap = donor.getBestCmap()
        dupm = donor["head"].unitsPerEm
        scale = upm / dupm
        gs = donor.getGlyphSet()
        still = []
        for cp in want:
            gname = dcmap.get(cp)
            if not gname:
                still.append(cp)
                continue
            pen = TTGlyphPen(None)
            try:
                rec = DecomposingRecordingPen(gs)
                gs[gname].draw(rec)
                rec.replay(TransformPen(pen, (scale, 0, 0, scale, 0, 0)))
                g = pen.glyph()
            except Exception:
                still.append(cp)
                continue
            new = f"uni{cp:04X}" if cp <= 0xFFFF else f"u{cp:05X}"
            if new in glyf:
                new = new + ".m"
            glyf[new] = g
            adv, lsb = donor["hmtx"][gname]
            g.recalcBounds(glyf)
            hmtx[new] = (round(adv * scale), getattr(g, "xMin", 0) or 0)
            for t in base["cmap"].tables:
                if t.isUnicode():
                    if cp > 0xFFFF and t.format != 12:
                        continue
                    t.cmap[cp] = new
            added += 1
        want = still
    # нужен формат 12 для символов вне BMP
    if not any(t.format == 12 for t in base["cmap"].tables):
        from fontTools.ttLib.tables._c_m_a_p import CmapSubtable
        t12 = CmapSubtable.newSubtable(12)
        t12.platformID, t12.platEncID, t12.language = 3, 10, 0
        t12.cmap = {}
        for t in base["cmap"].tables:
            if t.isUnicode():
                t12.cmap.update(t.cmap)
        base["cmap"].tables.append(t12)
        # повторно добавим всё, что вне BMP
        for gname in order:
            m = re.match(r"u([0-9A-F]{5})", gname)
            if m:
                t12.cmap[int(m.group(1), 16)] = gname
    order = list(glyf.glyphOrder)
    base.setGlyphOrder(order)
    base["maxp"].numGlyphs = len(order)
    base.save(path)
    print(path, "added", added, "missing left", len(want))

if __name__ == "__main__":
    for p in sys.argv[1:]:
        merge(p, bold=("Bold" in p or "SemiBold" in p))
