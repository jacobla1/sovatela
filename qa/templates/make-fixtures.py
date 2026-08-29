#!/usr/bin/env python3
"""Build the template fixtures the release checklist opens by hand.

A template is the one input this application copies *into* documents the user
sends to other people, so the checks that refuse a hostile one are the
substance of the feature — and each of them has been walked past at least
once. The unit tests cover the refusals; these cover the thing no automated
check in this repository can see, which is what Microsoft Word does with the
file afterwards.

`fetching-section.docx` is the one that matters. It carries an INCLUDEPICTURE
field in its section properties rather than in a header, which is the location
`load()` used to copy out of a template without checking. A document generated
from it fetched the URL when Word opened it — on the recipient's machine, with
no interaction beyond opening the file and updating fields.

    python3 qa/templates/make-fixtures.py [out-dir]      # default: ./out

See docs/release/QA-CHECKLIST.md § Templates for what to do with them.
"""
import os
import sys
import zipfile

W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
R = 'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'
STY = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"
HDR = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"
FTR = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"
MAIN = "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"

# Loopback on purpose. A fixture that reaches a host somebody else controls is
# a fixture that tells somebody else when you ran your release checklist.
BEACON = "http://127.0.0.1:8731/beacon.png"
FIELD = f'<w:fldSimple w:instr=" INCLUDEPICTURE &quot;{BEACON}&quot; \\d "/>'

RELS_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
REL_BASE = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"


def content_types(overrides):
    body = "".join(f'<Override PartName="/{p}" ContentType="{c}"/>' for p, c in overrides)
    return ('<?xml version="1.0"?>'
            '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
            f'<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
            f'<Default Extension="xml" ContentType="application/xml"/>{body}</Types>')


def heading(sid, name, level, half_points, colour):
    return (f'<w:style w:type="paragraph" w:styleId="{sid}"><w:name w:val="{name}"/>'
            f'<w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:outlineLvl w:val="{level}"/>'
            f'<w:spacing w:before="360" w:after="120"/></w:pPr><w:rPr>'
            f'<w:rFonts w:ascii="Trebuchet MS" w:hAnsi="Trebuchet MS"/><w:b/>'
            f'<w:color w:val="{colour}"/><w:sz w:val="{half_points}"/></w:rPr></w:style>')


# Real formatting, not name-only stubs. A template that defines a style id and
# gives it no properties produces a document that looks exactly like one built
# from no template at all — which reads as "the template was ignored" and costs
# an hour working out that it was not.
STYLES = (f'<?xml version="1.0"?><w:styles {W}>'
          '<w:docDefaults><w:rPrDefault><w:rPr>'
          '<w:rFonts w:ascii="Georgia" w:hAnsi="Georgia"/><w:sz w:val="24"/>'
          '</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr>'
          '<w:spacing w:after="160" w:line="288" w:lineRule="auto"/>'
          '</w:pPr></w:pPrDefault></w:docDefaults>'
          '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>'
          + heading("Heading1", "heading 1", 0, "44", "7A1F2B")
          + heading("Heading2", "heading 2", 1, "32", "1F4E5F")
          + heading("Heading3", "heading 3", 2, "26", "4A4A4A")
          + '<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/>'
            '<w:basedOn w:val="Normal"/><w:pPr><w:ind w:left="720"/></w:pPr></w:style>'
          + '</w:styles>')

HEADER = (f'<?xml version="1.0"?><w:hdr {W}><w:p><w:pPr><w:jc w:val="right"/></w:pPr>'
          '<w:r><w:rPr><w:b/><w:color w:val="7A1F2B"/></w:rPr>'
          '<w:t>ACME LTD — HOUSE STYLE</w:t></w:r></w:p></w:hdr>')
FOOTER = (f'<?xml version="1.0"?><w:ftr {W}><w:p><w:pPr><w:jc w:val="center"/></w:pPr>'
          '<w:r><w:rPr><w:sz w:val="18"/></w:rPr><w:t>Confidential — page </w:t></w:r>'
          '<w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p></w:ftr>')


def build(path, *, field_in_header=False, field_in_section=False, absolute_targets=False,
          footer_first=False):
    header = HEADER.replace("</w:p></w:hdr>", f"{FIELD}</w:p></w:hdr>") if field_in_header else HEADER
    refs = ('<w:footerReference r:id="rId5" w:type="default"/>'
            '<w:headerReference r:id="rId6" w:type="default"/>') if footer_first else (
           '<w:headerReference r:id="rId6" w:type="default"/>'
           '<w:footerReference r:id="rId5" w:type="default"/>')
    doc = (f'<?xml version="1.0"?><w:document {W} {R}><w:body>'
           '<w:p><w:r><w:t>House style.</w:t></w:r></w:p><w:sectPr>'
           f'{FIELD if field_in_section else ""}{refs}'
           '<w:pgSz w:w="12240" w:h="15840"/>'
           '<w:pgMar w:top="1985" w:right="1701" w:bottom="1985" w:left="1701"/>'
           '</w:sectPr></w:body></w:document>')
    prefix = "/word/" if absolute_targets else ""
    rels = (f'<?xml version="1.0"?><Relationships xmlns="{RELS_NS}">'
            f'<Relationship Id="rId6" Type="{REL_BASE}/header" Target="{prefix}header1.xml"/>'
            f'<Relationship Id="rId5" Type="{REL_BASE}/footer" Target="{prefix}footer1.xml"/>'
            '</Relationships>')
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", content_types([
            ("word/document.xml", MAIN), ("word/styles.xml", STY),
            ("word/header1.xml", HDR), ("word/footer1.xml", FTR)]))
        z.writestr("_rels/.rels", f'<?xml version="1.0"?><Relationships xmlns="{RELS_NS}">'
                   f'<Relationship Id="rId1" Type="{REL_BASE}/officeDocument" Target="word/document.xml"/>'
                   '</Relationships>')
        z.writestr("word/document.xml", doc)
        z.writestr("word/styles.xml", STYLES)
        z.writestr("word/_rels/document.xml.rels", rels)
        z.writestr("word/header1.xml", header)
        z.writestr("word/footer1.xml", FOOTER)


FIXTURES = [
    ("house-style.docx", dict(),
     "ACCEPTED — and generated documents come out in Georgia on US Letter, "
     "with the running header and the page-number footer"),
    ("absolute-targets.docx", dict(absolute_targets=True),
     "ACCEPTED, with the header and footer still present — an absolute target "
     "is a part name from the package root, and reading it as a relative one "
     "dropped both silently"),
    ("footer-reference-first.docx", dict(footer_first=True, absolute_targets=True),
     "ACCEPTED — a footer reference written before the header one used to be "
     "copied through unexamined, leaving a dangling id that refused the whole "
     "template"),
    ("fetching-header.docx", dict(field_in_header=True),
     "REFUSED, naming INCLUDEPICTURE"),
    ("fetching-section.docx", dict(field_in_section=True),
     "REFUSED, naming INCLUDEPICTURE — this is the one that reached Word"),
]

if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "out")
    os.makedirs(out, exist_ok=True)
    for name, opts, expected in FIXTURES:
        build(os.path.join(out, name), **opts)
        print(f"  {name:28} {expected}")
    print(f"\nwrote {len(FIXTURES)} fixtures to {out}")
    print("Listener for the fetching ones:  python3 -m http.server 8731")
