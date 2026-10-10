import re, pathlib

pat = re.compile(r'"(?:[^"\\]|\\.)*"')
seen = {}
for p in pathlib.Path('src').rglob('*.rs'):
    for i, line in enumerate(p.read_text(encoding='utf-8').splitlines(), 1):
        for m in pat.finditer(line):
            for ch in m.group(0):
                o = ord(ch)
                if o > 126 and not ('ç' <= ch <= 'ü' or 'Ç' <= ch <= 'Ü' or ch in 'ıİğĞşŞ'):
                    seen.setdefault(ch, []).append(f"{p}:{i}")
for ch, locs in sorted(seen.items(), key=lambda kv: -len(kv[1])):
    print(f"U+{ord(ch):04X} {ch.encode('unicode_escape').decode()} x{len(locs)}  e.g. {locs[0]}")
