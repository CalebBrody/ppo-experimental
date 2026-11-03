"""Read the team's spreadsheet without modifying it; cache public paste JSON.

Only the Python standard library is required. Incomplete sets are excluded by
default; conventional 252-EV pastes are rejected, not scaled.
"""
import argparse
import concurrent.futures
import hashlib
import json
from pathlib import Path
import re
import urllib.request
import xml.etree.ElementTree as ET
import zipfile

NS = {'m': 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}

def rows(path):
    with zipfile.ZipFile(path) as z:
        strings = [''.join(t.text or '' for t in el.findall('.//m:t', NS))
                   for el in ET.fromstring(z.read('xl/sharedStrings.xml'))]
        root = ET.fromstring(z.read('xl/worksheets/sheet1.xml'))
        records = []
        for row in root.findall('.//m:row', NS):
            cells = {}
            for c in row:
                v = c.find('m:v', NS)
                val = v.text if v is not None else ''
                if c.get('t') == 's':
                    val = strings[int(val)]
                elif c.get('t') == 'inlineStr':
                    val = ''.join(t.text or '' for t in c.findall('.//m:t', NS))
                cells[re.sub(r'\d', '', c.get('r'))] = val
            records.append(cells)
        return [{name: row.get(col, '') for col, name in records[0].items()}
                for row in records[1:]]

def pack(paste):
    result, assumptions = [], []
    for block in re.split(r'\n\s*\n', paste.strip()):
        lines = [x.strip() for x in block.splitlines() if x.strip()]
        header, *rest = lines
        name, _, item = header.partition(' @ ')
        gender = ''
        if name.endswith((' (M)', ' (F)')):
            gender, name = name[-2], name[:-4]
        match = re.search(r'\(([^()]+)\)$', name)
        species = match.group(1) if match else name
        ability = next((x[9:] for x in rest if x.startswith('Ability: ')), '')
        if not ability:
            raise ValueError('missing ability: ' + species)
        moves = [x[2:] for x in rest if x.startswith('- ')]
        if len(moves) != 4:
            raise ValueError('expected four moves: ' + species)
        nature = next((x[:-7] for x in rest if x.endswith(' Nature')), 'Serious')
        ev = next((x.split(': ', 1)[1] for x in rest if x.startswith(('EVs: ', 'SPs: '))), '')
        points = [0] * 6
        for value, stat in re.findall(r'(\d+)\s+(HP|Atk|Def|SpA|SpD|Spe)', ev):
            points[['HP', 'Atk', 'Def', 'SpA', 'SpD', 'Spe'].index(stat)] = int(value)
        if max(points) > 32 or sum(points) > 66:
            raise ValueError('non-Champions spread: ' + species)
        if not ev:
            assumptions.append(species + ': missing spread, using 0 points')
        if nature == 'Serious' and not any(x.endswith(' Nature') for x in rest):
            assumptions.append(species + ': missing nature, using Serious')
        result.append('|'.join([species, '', item, ability, ','.join(moves), nature,
                               ','.join(map(str, points)), gender, '', '', '50', '']))
    if len(result) != 6:
        raise ValueError('expected six Pokemon')
    return ']'.join(result), assumptions

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--xlsx', type=Path, default=Path('../../vgc.xlsx'))
    ap.add_argument('--out', type=Path, default=Path('data'))
    ap.add_argument('--limit', type=int, default=0)
    ap.add_argument('--workers', type=int, default=4)
    ap.add_argument('--allow-incomplete', action='store_true', help='Opt into zero-point / Serious defaults for missing data')
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    cache = args.out / 'pastes'
    cache.mkdir(exist_ok=True)
    source = rows(args.xlsx)
    if args.limit:
        source = source[:args.limit]
    def fetch(row):
        url = row.get('Pokepaste URL', '')
        try:
            match = re.fullmatch(r'https://pokepast\.es/([0-9a-f]{16})/?', url)
            if not match:
                raise ValueError('invalid Pokepaste URL')
            dest = cache / (match.group(1) + '.json')
            if not dest.exists():
                req = urllib.request.Request(url.rstrip('/') + '/json', headers={'User-Agent': 'Model1-team-import/0.1'})
                with urllib.request.urlopen(req, timeout=30) as response:
                    payload = response.read()
                json.loads(payload)
                dest.write_bytes(payload)
            data = json.loads(dest.read_text(encoding='utf-8'))
            packed, assumptions = pack(data['paste'])
            if assumptions and not args.allow_incomplete:
                raise ValueError('incomplete set: ' + '; '.join(assumptions))
            return dict(id=row['Team ID'], url=url, packed=packed, assumptions=assumptions)
        except Exception as ex:
            return dict(id=row.get('Team ID'), url=url, error=str(ex))
    with concurrent.futures.ThreadPoolExecutor(args.workers) as pool:
        imported = list(pool.map(fetch, source))
    seen, train, evaluation = set(), [], []
    for row in imported:
        if 'error' in row:
            continue
        digest = hashlib.sha256(row['packed'].encode()).hexdigest()
        if digest in seen:
            continue
        seen.add(digest)
        (evaluation if int(digest[:8], 16) % 10 == 0 else train).append(row['packed'])
    for name, teams in [('teams.txt', train), ('eval-teams.txt', evaluation)]:
        (args.out / name).write_text('\n'.join(teams) + '\n', encoding='utf-8')
    report = dict(source=str(args.xlsx.resolve()), source_sha256=hashlib.sha256(args.xlsx.read_bytes()).hexdigest(),
                  rows=len(source), train=len(train), evaluation=len(evaluation), records=imported)
    (args.out / 'import-report.json').write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding='utf-8')
    print(f"Imported {len(train)} training and {len(evaluation)} evaluation teams; "
          f"{sum('error' in r for r in imported)} rejected. See data/import-report.json.")
    if not train:
        raise SystemExit('No training teams imported')

if __name__ == '__main__':
    main()
