"""Assembly instructions for a split part.

From the split's seams (which pieces meet, with which joints): an order that
starts with the largest piece and adds one neighbour at a time, the hardware
list with the tools it needs, a self-contained HTML guide with a picture of
every step, and an exploded view (instances of the pieces, offset along the
way each one goes on; the pieces themselves are not moved).

Preparation comes first: heat-set inserts and press-fit pins go into their
pieces before those pieces are joined (the lesson "Screw bosses and heat-set
inserts" explains the insert).
"""

from __future__ import annotations

import base64
import html
import json
import os
import tempfile
from dataclasses import dataclass
from typing import Optional

import numpy as np

from .kernel.base import v_scale

HEX_KEY = {'M2': '1.5 mm', 'M2.5': '2 mm', 'M3': '2.5 mm', 'M4': '3 mm', 'M5': '4 mm'}


@dataclass
class Step:
    number: int
    title: str
    text: str
    piece: Optional[int] = None          # the piece this step adds (index)
    direction: Optional[list] = None     # the way it goes on
    hardware: list = None

    def public(self) -> dict:
        return {'number': self.number, 'title': self.title, 'text': self.text, 'piece': self.piece, 'direction': self.direction, 'hardware': self.hardware or []}


def _split_of(doc, group_id: str) -> tuple[dict, list[str]]:
    g = doc.nodes[group_id]
    split = (g.robot or {}).get('print_split')
    if not split:
        raise ValueError(f'{g.name} is not a split for printing (no print_split record)')
    pieces = sorted((c for c in g.children if (doc.nodes[c].robot or {}).get('print_piece')), key=lambda c: doc.nodes[c].robot['print_piece']['index'])
    return split, pieces


def plan_assembly(doc, group_id: str) -> dict:
    split, piece_ids = _split_of(doc, group_id)
    k = doc.kernel
    volumes = [k.mass_properties(doc.nodes[p].body).volume for p in piece_ids]
    names = [doc.nodes[p].name for p in piece_ids]
    seams = split['seams']
    steps: list[Step] = []
    n = 0
    # Preparation per piece: inserts into the minus side, pins pressed into the minus side.
    prep = {}
    for s in seams:
        for j in s['joints']:
            if j['kind'] in ('insert_screw', 'dowel'):
                prep.setdefault((s['minus'], j['kind']), []).append(j)
    for (piece, kind), js in sorted(prep.items()):
        n += 1
        if kind == 'insert_screw':
            size = _size(js[0])
            steps.append(Step(n, f'Inserts into {names[piece]}',
                              f'Press {len(js)} {size} heat-set insert(s) into the pockets on the cut face of {names[piece]} with a soldering iron at about 220–240 °C '
                              f'(PLA). Push straight and slowly until each is flush; let them cool before the next step.', piece,
                              hardware=[{'item': 'heat-set insert', 'size': size, 'count': len(js)}]))
        else:
            steps.append(Step(n, f'Pins into {names[piece]}',
                              f'Press {len(js)} steel dowel pin(s) into the tight holes on the cut face of {names[piece]} (a vice or a gentle tap). '
                              'They stand proud and locate the next piece.', piece,
                              hardware=[{'item': 'steel dowel pin', 'size': _pin(js[0]), 'count': len(js)}]))
    # Joining: largest piece first, then neighbours by seam, breadth first.
    order = [int(np.argmax(volumes))]
    placed = set(order)
    todo = list(seams)
    joins = []
    while len(placed) < len(piece_ids):
        grew = False
        for s in list(todo):
            a, b = s['minus'], s['plus']
            if (a in placed) != (b in placed):
                new = b if a in placed else a
                links = [x for x in seams if new in (x['minus'], x['plus']) and ({x['minus'], x['plus']} - {new}) <= placed]
                joins.append((new, links))
                placed.add(new)
                order.append(new)
                for x in links:
                    if x in todo:
                        todo.remove(x)
                grew = True
                break
        if not grew:
            missing = [names[i] for i in range(len(piece_ids)) if i not in placed]
            raise ValueError(f'these pieces share no seam with the rest: {", ".join(missing)}')
    n += 1
    steps.append(Step(n, f'Start with {names[order[0]]}', f'Lay {names[order[0]]} down with its cut faces up or out: the other pieces join onto it.', order[0]))
    for new, links in joins:
        n += 1
        tabs = [j for x in links for j in x['joints'] if j['kind'] == 'dovetail']
        screws = [j for x in links for j in x['joints'] if j['kind'] == 'insert_screw']
        pins = [j for x in links for j in x['joints'] if j['kind'] == 'dowel']
        direction = _direction(doc, piece_ids, new, links, split)
        how = []
        jigsaw = [j for j in tabs if any('jigsaw' in note for note in j['notes'])]
        if jigsaw:
            how.append(f'lower it straight in so its {len(jigsaw)} dovetail tab(s) drop into the matching slots')
        elif tabs:
            how.append('slide it along the dovetail rail from the open end')
        else:
            how.append('push it straight onto the cut face')
        if pins:
            how.append(f'{len(pins)} pin(s) line it up')
        text = f'Fit {names[new]}: ' + '; '.join(how) + '. Check the seam closes all the way, with no gap.'
        hw = []
        if screws:
            size = _size(screws[0])
            lengths = sorted({_screw_len(j) for j in screws})
            text += (f' Then drive {len(screws)} {size} screw(s) ({", ".join(lengths)}) through the counterbores or side pockets into the inserts with a '
                     f'{HEX_KEY.get(size, "hex")} hex key. Snug, not tight: the insert holds far more than a plastic thread, but the head bears on plastic.')
            for L in lengths:
                hw.append({'item': 'socket head screw', 'size': L, 'count': sum(1 for j in screws if _screw_len(j) == L)})
        steps.append(Step(n, f'Add {names[new]}', text, new, direction, hw))
    tools = []
    if any(j['kind'] == 'insert_screw' for s in seams for j in s['joints']):
        sizes = sorted({_size(j) for s in seams for j in s['joints'] if j['kind'] == 'insert_screw'})
        tools += ['soldering iron with an insert tip (220–240 °C for PLA)'] + [f'{HEX_KEY.get(z, "hex")} hex key ({z})' for z in sizes]
    if any(j['kind'] == 'dowel' for s in seams for j in s['joints']):
        tools.append('vice or small hammer (pressing pins)')
    return {'group': group_id, 'pieces': [{'index': i, 'node': pid, 'name': names[i], 'volume_mm3': round(volumes[i], 1)} for i, pid in enumerate(piece_ids)],
            'order': order, 'steps': [s.public() for s in steps], 'hardware': split['hardware'], 'tools': tools}


def _size(j) -> str:
    return j['spec'].get('size', 'M3')


def _screw_len(j) -> str:
    return f"{j['spec']['size']} × {j['spec']['screw_length_mm']:g} mm"


def _pin(j) -> str:
    return next((h['size'] for h in j.get('hardware', []) if h['item'] == 'steel dowel pin'), f"Ø{j['spec']['diameter_mm']:g} mm")


def _direction(doc, piece_ids, new, links, split) -> list:
    """The way a piece goes on: along its dovetail tabs or rail, else straight
    along the seam normal, away from the piece it joins."""
    for x in links:
        for j in x['joints']:
            if j['kind'] == 'dovetail':
                return [float(v) for v in j['spec']['along']]
    x = links[0]
    n = x['normal']
    return [float(v) for v in n] if new == x['plus'] else [-float(v) for v in n]


def exploded_offsets(doc, plan: dict, spacing: float = 35.0) -> dict[str, tuple]:
    """Offsets for an exploded view: each added piece moves back along its way on,
    further for later steps."""
    offsets = {}
    depth = 0
    for s in plan['steps']:
        if s['direction'] is not None and s['piece'] is not None:
            depth += 1
            pid = plan['pieces'][s['piece']]['node']
            offsets[pid] = tuple(v_scale(tuple(s['direction']), spacing * (1 + 0.5 * (depth - 1))))
    for p in plan['pieces']:
        offsets.setdefault(p['node'], (0.0, 0.0, 0.0))
    return offsets


def add_exploded_view(ops, plan: dict, name: Optional[str] = None) -> str:
    """Instances of the pieces, offset: an exploded view that follows the pieces (one undo step)."""
    from .commands import AddNodes
    from .document import Node, Transform
    doc = ops.doc
    group = Node(doc.new_id(), 'group', doc.unique_name(name or f'{doc.nodes[plan["group"]].name}: exploded'))
    nodes = [group]
    for pid, off in exploded_offsets(doc, plan).items():
        src = doc.nodes[pid]
        nodes.append(Node(doc.new_id(), 'instance', doc.unique_name(f'{src.name} (exploded)'), source=pid, transform=Transform(off), material=src.material, parent=group.id))
    ops.stack.push(AddNodes('Exploded view', nodes))
    return group.id


def write_guide(doc, plan: dict, out_dir: str, title: Optional[str] = None, images: bool = True) -> str:
    """assembly.json and a self-contained assembly.html (a picture per step)."""
    from .io.snapshot import render
    os.makedirs(out_dir, exist_ok=True)
    k = doc.kernel
    offsets = exploded_offsets(doc, plan)
    pics = {}
    if images:
        from .commands import Ops
        from .document import Document
        # Draw on a scratch document so the user's document is untouched.
        scratch = Document()
        sops = Ops(scratch)
        ids = {}
        for p in plan['pieces']:
            ids[p['node']] = sops._new('piece', k.copy(doc.nodes[p['node']].body), p['name'])
        present = set()
        palette = [(0.95, 0.6, 0.25), (0.3, 0.55, 0.9), (0.4, 0.8, 0.45), (0.85, 0.4, 0.6), (0.7, 0.65, 0.3), (0.5, 0.45, 0.85)]
        for s in plan['steps']:
            if s['piece'] is None or s['direction'] is None and not s['title'].startswith('Start'):
                continue
            pid = plan['pieces'][s['piece']]['node']
            present.add(pid)
            shown = []
            for q in present:
                sid = ids[q]
                body = doc.nodes[q].body
                off = offsets[q] if q == pid and s['direction'] is not None else (0.0, 0.0, 0.0)
                scratch.nodes[sid].body = k.transform(body, translation=off)
                idx = next(i for i, p in enumerate(plan['pieces']) if p['node'] == q)
                scratch.nodes[sid].color = palette[idx % len(palette)] if q == pid else (0.72, 0.72, 0.74)
                shown.append(sid)
            scratch.mesh_cache.clear()
            path = os.path.join(tempfile.mkdtemp(), f'step-{s["number"]}.png')
            render(scratch, path, ids=shown, size=(900, 620), tolerance=0.5, edges=False, view=(-0.9, -1.2, 1.0))
            with open(path, 'rb') as f:
                pics[s['number']] = base64.b64encode(f.read()).decode()
    with open(os.path.join(out_dir, 'assembly.json'), 'w') as f:
        json.dump(plan, f, indent=1)
    title = title or f'Assembling {doc.nodes[plan["group"]].name}'
    rows = ''.join(f'<li>{h["count"]} × {html.escape(h["item"])} {html.escape(h["size"])}</li>' for h in plan['hardware']) or '<li>none</li>'
    tools = ''.join(f'<li>{html.escape(t)}</li>' for t in plan['tools']) or '<li>none</li>'
    steps = ''.join(
        f'<section class="step"><h2>{s["number"]}. {html.escape(s["title"])}</h2><p>{html.escape(s["text"])}</p>'
        + (f'<img alt="step {s["number"]}" src="data:image/png;base64,{pics[s["number"]]}">' if s['number'] in pics else '') + '</section>'
        for s in plan['steps'])
    page = f'''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(title)}</title><style>
:root{{--bg:#fafaf8;--ink:#1d2025;--muted:#5b6270;--card:#fff;--line:#e3e2de}}
@media (prefers-color-scheme:dark){{:root{{--bg:#16181c;--ink:#e8e8e6;--muted:#a4a9b3;--card:#1f2228;--line:#30343c}}}}
body{{margin:0;background:var(--bg);color:var(--ink);font:16px/1.5 system-ui,-apple-system,Segoe UI,sans-serif}}
main{{max-width:880px;margin:0 auto;padding:24px 16px}} h1{{font-size:1.6rem;margin:0 0 4px}} .muted{{color:var(--muted)}}
.cols{{display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:16px;margin:16px 0}}
.card,.step{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:14px 18px}} .step{{margin:14px 0}}
.step h2{{font-size:1.1rem;margin:0 0 6px}} img{{width:100%;height:auto;border-radius:6px;margin-top:8px;background:#20242a}}
</style></head><body><main><h1>{html.escape(title)}</h1>
<p class="muted">{len(plan["pieces"])} printed pieces. Order: largest piece first, then each neighbour; inserts and pins go in before their piece is joined.</p>
<div class="cols"><div class="card"><h3>Hardware</h3><ul>{rows}</ul></div><div class="card"><h3>Tools</h3><ul>{tools}</ul></div></div>
{steps}</main></body></html>'''
    path = os.path.join(out_dir, 'assembly.html')
    with open(path, 'w') as f:
        f.write(page)
    return path
