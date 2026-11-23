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

sources = {
    'NotoSansCJK-Regular.ttc': 'NotoSansCJK',
    'NotoSansCJK-Bold.ttc': 'NotoSansCJKBold',
}
for filename in sources:
    if not pathlib.Path('/usr/share/fonts/noto-cjk', filename).exists():
        sys.exit('system Noto Sans CJK collection not found: ' + filename)
outdir = root / 'egui-app/assets'
outdir.mkdir(parents=True, exist_ok=True)
faces = [
    ('NotoSansCJK-Regular.ttc', 'NotoSansCJKtc-subset.otf', 3),
    ('NotoSansCJK-Regular.ttc', 'NotoSansCJKsc-subset.otf', 2),
    ('NotoSansCJK-Bold.ttc', 'NotoSansCJKtc-Bold-subset.otf', 3),
    ('NotoSansCJK-Bold.ttc', 'NotoSansCJKsc-Bold-subset.otf', 2),
]
for source, name, index in faces:
    out = outdir / name
    subprocess.run(
        [sys.executable, '-m', 'fontTools.subset', '/usr/share/fonts/noto-cjk/' + source,
         f'--font-number={index}',
         f'--text-file={root}/scripts/i18n/subset-chars.txt',
         '--layout-features=*', '--no-hinting', '--desubroutinize',
         f'--output-file={out}'],
        check=True,
    )
    print(name, out.stat().st_size, 'bytes')
