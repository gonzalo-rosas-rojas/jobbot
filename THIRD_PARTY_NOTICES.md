# Third-party notices

What the binary and the page carry that was not written in this repository, plus the assets that were, so that
every byte served by the panel has a row here. One row per asset in `src/panel/assets/` (the rule is in its
`README.md`): no row, no asset. The sha256 is of the file as committed (the SVGs are written without a trailing line
end, so the hash does not depend on the checkout's line endings). `own` means drawn or written here, under the
repository's own terms.

The page's JavaScript dependencies (three.js, anime.js, bundled by esbuild into `src/panel/page.html`) are listed in
`panel-src/package.json` with exact versions and in `panel-src/package-lock.json` with their integrity hashes; their
licence texts are MIT. The bundle is minified with `legalComments: none`, so this file is where their notices live.

## Assets served by the binary (`/a/<launch-id>/<name>`)

| Name | Type | Origin | Licence | Source URL | sha256 |
|---|---|---|---|---|---|
| `barlow-condensed-700.woff2` | `font/woff2` | Barlow Condensed Bold (700). Copyright 2017 The Barlow Project Authors. Origin: google/fonts commit 2eb0b48d5f760f62e286216f0859a8c540dbc1bd, `ofl/barlowcondensed/BarlowCondensed-Bold.ttf` (git blob `9e480f467d703009e3abfce290b2ec2b7d24e188`); subset to U+0020-007E, U+00A0-00FF, U+0100-017F, U+2000-206F, U+20A0-20CF and U+2190-21FF with pyftsubset (fonttools 4.66.1) and accepted by ots 9.2.0; details and hashes in `panel-src/assets/fonts/VENDORED.md` | OFL-1.1 | https://raw.githubusercontent.com/google/fonts/2eb0b48d5f760f62e286216f0859a8c540dbc1bd/ofl/barlowcondensed/BarlowCondensed-Bold.ttf | `fdd760515b0e74f20baad8d00d6317b51c83f54f26755232ddbc59bcabf0f060` |
| `geist-mono-400.woff2` | `font/woff2` | Geist Mono Regular (400). Copyright (c) 2023 Vercel, in collaboration with basement.studio. Origin: vercel/geist-font tag v1.7.2 (commit a73329da8fc62afc917f796555202e4997f79b7c), `fonts/GeistMono/ttf/GeistMono-Regular.ttf` (git blob `643e6ba0731882a61296da368cb0d6362290b882`); subset to U+0020-007E, U+00A0-00FF, U+0100-017F, U+2000-206F, U+20A0-20CF and U+2190-21FF with pyftsubset (fonttools 4.66.1) and accepted by ots 9.2.0; details and hashes in `panel-src/assets/fonts/VENDORED.md` | OFL-1.1 | https://raw.githubusercontent.com/vercel/geist-font/a73329da8fc62afc917f796555202e4997f79b7c/fonts/GeistMono/ttf/GeistMono-Regular.ttf | `2702e93f28bcb81633d91a18fc5ce31f6cc61bd0f386347a645eb478b7f655e9` |
| `instrument-serif-400italic.woff2` | `font/woff2` | Instrument Serif Italic (400). Copyright 2022 The Instrument Serif Project Authors. Origin: google/fonts commit 2eb0b48d5f760f62e286216f0859a8c540dbc1bd, `ofl/instrumentserif/InstrumentSerif-Italic.ttf` (git blob `192c3eda9864241ad280e9bcc84e258c80a4a588`); subset to U+0020-007E, U+00A0-00FF, U+0100-017F, U+2000-206F, U+20A0-20CF and U+2190-21FF with pyftsubset (fonttools 4.66.1) and accepted by ots 9.2.0; details and hashes in `panel-src/assets/fonts/VENDORED.md` | OFL-1.1 | https://raw.githubusercontent.com/google/fonts/2eb0b48d5f760f62e286216f0859a8c540dbc1bd/ofl/instrumentserif/InstrumentSerif-Italic.ttf | `bde7dc8f24f537bca4535f1007f2f8f7ba6059b6584e961c869581a476e28d81` |
| `inter-400.woff2` | `font/woff2` | Inter Regular (400, optical size 14). Copyright 2020 The Inter Project Authors. Origin: google/fonts commit 2eb0b48d5f760f62e286216f0859a8c540dbc1bd, `ofl/inter/Inter[opsz,wght].ttf` (git blob `047c92f6e2212473dc436020afed689527076d44`), instanced at wght=400 opsz=14 with fonttools varLib.instancer; subset to U+0020-007E, U+00A0-00FF, U+0100-017F, U+2000-206F, U+20A0-20CF and U+2190-21FF with pyftsubset (fonttools 4.66.1) and accepted by ots 9.2.0; details and hashes in `panel-src/assets/fonts/VENDORED.md` | OFL-1.1 | https://raw.githubusercontent.com/google/fonts/2eb0b48d5f760f62e286216f0859a8c540dbc1bd/ofl/inter/Inter[opsz,wght].ttf | `a656f4e64840b2a335ca1728f39c5274d88062a611a5672bcba3c62cd496bbd9` |
| `inter-600.woff2` | `font/woff2` | Inter SemiBold (600, optical size 14). Copyright 2020 The Inter Project Authors. Origin: google/fonts commit 2eb0b48d5f760f62e286216f0859a8c540dbc1bd, `ofl/inter/Inter[opsz,wght].ttf` (git blob `047c92f6e2212473dc436020afed689527076d44`), instanced at wght=600 opsz=14 with fonttools varLib.instancer; subset to U+0020-007E, U+00A0-00FF, U+0100-017F, U+2000-206F, U+20A0-20CF and U+2190-21FF with pyftsubset (fonttools 4.66.1) and accepted by ots 9.2.0; details and hashes in `panel-src/assets/fonts/VENDORED.md` | OFL-1.1 | https://raw.githubusercontent.com/google/fonts/2eb0b48d5f760f62e286216f0859a8c540dbc1bd/ofl/inter/Inter[opsz,wght].ttf | `b73ab1be14e4e8f1b92c3eecb3295175d613bb7b7075b94e8f7b959df7d4a22a` |

### Font licence

The five fonts are under the SIL Open Font License, Version 1.1 (SPDX `OFL-1.1`, <https://openfontlicense.org>). They are
redistributed here as subsets, unmodified in design and without being sold on their own. The full text, with each
copyright holder's notice at its head, is kept next to the fonts in `panel-src/assets/fonts/licenses/`: `OFL-barlow-condensed.txt`
(Barlow Condensed), `OFL-geist-mono.txt` (Geist Mono), `OFL-instrument-serif.txt` (Instrument Serif) and `OFL-inter.txt` (both
Inter weights). The sha256 of each upstream licence file, and of each original font before subsetting, is in
`panel-src/assets/fonts/VENDORED.md`; the sha256 in the table above is of the subset file exactly as it sits in
`src/panel/assets/`.

## Libraries bundled into the page

| Package | Version | Licence | Source |
|---|---|---|---|
| three | 0.186.1 | MIT | https://github.com/mrdoob/three.js |
| animejs | 4.5.0 | MIT | https://github.com/juliangarnier/anime |

MIT licence text (three.js: Copyright 2010-2026 three.js authors; anime.js: Copyright (c) 2025 Julian Garnier):

> Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated
> documentation files (the "Software"), to deal in the Software without restriction, including without limitation
> the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to
> permit persons to whom the Software is furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or substantial portions of
> the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO
> THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
> TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.
