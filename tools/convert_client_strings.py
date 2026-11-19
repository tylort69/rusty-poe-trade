#!/usr/bin/env python3
"""Convert upstream `client_strings.js` (JS object with regex literals) to JSON.

Regex literals become {"$re": "<pattern>", "flags": "<flags>"}.
Usage: python3 -I tools/convert_client_strings.py assets/data
"""
import json, re, sys, pathlib

LINE = re.compile(r"^\s*([A-Z0-9_]+)\s*:\s*(.*?),?\s*$")

def parse_js_string(s, i):
    q = s[i]; i += 1; out = []
    while s[i] != q:
        if s[i] == '\\':
            nxt = s[i + 1]
            out.append({'n': '\n', 't': '\t', '\\': '\\', "'": "'", '"': '"'}.get(nxt, nxt))
            i += 2
        else:
            out.append(s[i]); i += 1
    return ''.join(out), i + 1

def parse_value(s):
    s = s.strip()
    if s.startswith('/'):
        end = s.rfind('/')
        return {"$re": s[1:end], "flags": s[end + 1:].rstrip(',')}
    if s[0] in "'\"":
        v, _ = parse_js_string(s, 0)
        return v
    if s[0] == '[':
        items, i = [], 1
        while True:
            while s[i] in ' ,': i += 1
            if s[i] == ']': break
            v, i = parse_js_string(s, i)
            items.append(v)
        return items
    raise ValueError(s)

def convert(path):
    out = {}
    for line in path.read_text(encoding='utf-8').splitlines():
        if line.strip().startswith('//'): continue
        m = LINE.match(line)
        if m and m.group(2):
            out[m.group(1)] = parse_value(m.group(2))
    return out

root = pathlib.Path(sys.argv[1])
for js in sorted(root.glob('*/client_strings.js')):
    data = convert(js)
    js.with_suffix('.json').write_text(json.dumps(data, ensure_ascii=False, indent=1), encoding='utf-8')
    print(js.parent.name, len(data), 'keys')
