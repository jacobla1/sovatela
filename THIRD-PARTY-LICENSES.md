# Third-party licenses & notices

This application is distributed under its own **MIT License** (see `LICENSE`).
It bundles and links third-party software. This file reproduces the required
notices for the primary redistributed components and summarises the full
dependency graph. Nothing here is legal advice.

## License composition (audited)

Both dependency trees were scanned; **no GPL, LGPL, or AGPL obligations reach
this project's code**, and there are no copyleft terms that affect how it may
be distributed.

**npm graph — 151 packages:** MIT (130), Apache-2.0 (5), ISC (4),
Apache-2.0/MIT dual (5), BSD-2/3-Clause (4), MIT-0 (1), OFL-1.1 (1, the Inter
font), MPL-2.0-OR-Apache-2.0 (1, DOMPurify — taken under Apache-2.0).

**Cargo graph — 598 crates:** overwhelmingly `MIT OR Apache-2.0`. Notable:
- **MPL-2.0** (5 crates: `cssparser`, `cssparser-macros`, `selectors`,
  `dtoa-short`, `option-ext`), pulled in transitively via the web-view stack.
  MPL-2.0 is *file-level* weak copyleft: because these crates are consumed
  **unmodified**, the only obligation is to preserve their own notices, which
  distribution of the unmodified crates satisfies.
- **`r-efi`** is tri-licensed (`MIT OR Apache-2.0 OR LGPL-2.1-or-later`); it is
  used here under **MIT/Apache-2.0**, so the LGPL option does not apply.
- **Unicode-3.0** (Unicode data) and permissive BSD/ISC/Zlib/0BSD terms.

A complete, machine-generated per-package manifest should accompany any formal
binary release — see "Regenerating the full manifest" at the end.

## Primary redistributed components

| Component | Role | License | Copyright |
|---|---|---|---|
| Tauri (`@tauri-apps/*`, `tauri` crates) | App framework | Apache-2.0 OR MIT | © The Tauri Programme (Commons Conservancy) |
| Svelte | UI runtime (bundled) | MIT | © 2016–2025 Svelte Contributors |
| Vite | Build tooling | MIT | © Evan You & Vite contributors |
| marked | Markdown rendering (bundled) | MIT | © 2018+ MarkedJS; © 2011–2018 Christopher Jeffrey |
| DOMPurify | HTML sanitisation (bundled) | Apache-2.0 (elected) | © 2015 Mario Heiderich |
| Inter (`@fontsource-variable/inter`) | Bundled UI font | SIL OFL-1.1 | © 2016 The Inter Project Authors |
| Icons (`src/lib/Icon.svelte`) | UI line icons | ISC | Lucide Contributors / Feather (see below) |
| reqwest, keyring, pdf-extract, zip, … | Rust runtime crates | MIT OR Apache-2.0 | respective authors |

Full license texts for MIT-licensed components are covered by the single MIT
text below (the license permits this). Apache-2.0, BSD-2/3-Clause, ISC,
MPL-2.0, Zlib and Unicode-3.0 texts are the standard canonical versions,
available at the URLs noted and reproduced in each package's own distribution.

## Icons

The line icons in `src/lib/Icon.svelte` were authored for this project in the
style of, and with path data derived from, **Lucide** (https://lucide.dev),
which is ISC-licensed:

> ISC License. Copyright (c) for portions of Lucide are held by Cole Bemis
> 2013–2022 as part of Feather (MIT). All other copyright (c) for Lucide are
> held by Lucide Contributors 2022.

## Fonts — Inter (SIL Open Font License 1.1)

**"Inter" is a Reserved Font Name.** The font is bundled unmodified; this
application does not create a Modified Version and does not use the reserved
name for any derivative.

```
Copyright 2016 The Inter Project Authors (https://github.com/rsms/inter)

This Font Software is licensed under the SIL Open Font License, Version 1.1.
This license is copied below, and is also available with a FAQ at:
http://scripts.sil.org/OFL

-----------------------------------------------------------
SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007
-----------------------------------------------------------

PREAMBLE
The goals of the Open Font License (OFL) are to stimulate worldwide
development of collaborative font projects, to support the font creation
efforts of academic and linguistic communities, and to provide a free and
open framework in which fonts may be shared and improved in partnership
with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves. The
fonts, including any derivative works, can be bundled, embedded,
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works. The fonts and derivatives,
however, cannot be released under any other type of license. The
requirement for fonts to remain under this license does not apply
to any document created using the fonts or their derivatives.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such. This may
include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software components as
distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to a
new environment.

"Author" refers to any designer, engineer, programmer, technical
writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS
Permission is hereby granted, free of charge, to any person obtaining
a copy of the Font Software, to use, study, copy, merge, embed, modify,
redistribute, and sell modified and unmodified copies of the Font
Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components,
in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
redistributed and/or sold with any software, provided that each copy
contains the above copyright notice and this license. These can be
included either as stand-alone text files, human-readable headers or
in the appropriate machine-readable metadata fields within text or
binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
Name(s) unless explicit written permission is granted by the corresponding
Copyright Holder. This restriction only applies to the primary font name as
presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
Software shall not be used to promote, endorse or advertise any
Modified Version, except to acknowledge the contribution(s) of the
Copyright Holder(s) and the Author(s) or with their explicit written
permission.

5) The Font Software, modified or unmodified, in part or in whole,
must be distributed entirely under this license, and must not be
distributed under any other license. The requirement for fonts to
remain under this license does not apply to any document created
using the Font Software.

TERMINATION
This license becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM
OTHER DEALINGS IN THE FONT SOFTWARE.
```

## The MIT License (covers all MIT-licensed dependencies above)

Each MIT-licensed dependency is copyright its respective authors (see the
table and each package's own `LICENSE`). The MIT terms are:

```
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Other licenses referenced

Standard canonical texts, reproduced in each package's own distribution:

- **Apache-2.0** — https://www.apache.org/licenses/LICENSE-2.0
- **MPL-2.0** — https://www.mozilla.org/MPL/2.0/
- **ISC** — https://opensource.org/license/isc-license-txt
- **BSD-2-Clause / BSD-3-Clause** — https://opensource.org/licenses/BSD-3-Clause
- **Zlib** — https://opensource.org/license/zlib
- **Unicode-3.0** — https://www.unicode.org/license.txt

## Third-party services (not bundled)

The app connects, using the user's own API keys, to services governed by their
own terms — Z.ai GLM-5.2 via Scaleway, Mistral, OVHcloud AI Endpoints, Black
Forest Labs (FLUX), Linkup, and Qwant Staan — and, optionally, to a
user-operated **SearXNG** instance (AGPL-3.0). SearXNG is a separate service
reached over HTTP and is **not** bundled or linked, so its terms do not attach
to this application.

## Regenerating the full manifest

For a formal binary release, generate an exhaustive per-dependency manifest:

- **Rust:** `cargo install cargo-about && cargo about generate about.hbs`
  (run in `src-tauri/`)
- **JavaScript:** `npx license-checker-rseidelsohn --production --out THIRD-PARTY-JS.txt`

These enumerate every crate/package with its exact license text and copyright.
