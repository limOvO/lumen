#!/usr/bin/env python3
"""Build the subset CJK fonts used by the egui app.

Includes every character that appears in the shared i18n dictionaries (plus
printable ASCII and common CJK punctuation), so the egui build does not need to
load the 19 MB system Noto collection at runtime.

Needs fontTools:  python3 -m venv .cache/venv && .cache/venv/bin/pip install fonttools
Then:             .cache/venv/bin/python scripts/gen-fonts.py
"""
import json, pathlib, subprocess, sys

root = pathlib.Path(__file__).resolve().parent.parent
keys = json.loads((root / 'scripts/i18n/keys.json').read_text(encoding='utf-8'))
en = json.loads((root / 'scripts/i18n/en.json').read_text(encoding='utf-8'))
chars = set(''.join(keys) + ''.join(en.values()))
chars |= set(chr(c) for c in range(0x20, 0x7F))
chars |= set('·×—…「」『』（）：；，。、！？％／')
(root / 'scripts/i18n/subset-chars.txt').write_text(''.join(sorted(chars)), encoding='utf-8')

src = '/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc'
if not pathlib.Path(src).exists():
    sys.exit('system Noto Sans CJK collection not found: ' + src)
outdir = root / 'egui-app/assets'
outdir.mkdir(parents=True, exist_ok=True)
for name, index in (('NotoSansCJKtc-subset.otf', 3), ('NotoSansCJKsc-subset.otf', 2)):
    out = outdir / name
    subprocess.run(
        [sys.executable, '-m', 'fontTools.subset', src,
         f'--font-number={index}',
         f'--text-file={root}/scripts/i18n/subset-chars.txt',
         '--layout-features=*', '--no-hinting', '--desubroutinize',
         f'--output-file={out}'],
        check=True,
    )
    print(name, out.stat().st_size, 'bytes')
