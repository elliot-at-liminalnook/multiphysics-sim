"""Localisation-ready string table. `tr("key")` returns the English text
by default; drop a JSON file next to this module named `strings.<lang>.json`
and set `ROBOCAD_LANG` to use it."""

from __future__ import annotations

import json
import os

_EN = {
    "app.title": "robocad",
    "menu.file": "&File", "menu.edit": "&Edit", "menu.view": "&View", "menu.create": "&Create", "menu.modify": "&Modify", "menu.inspect": "&Inspect", "menu.print": "&Print", "menu.window": "&Window", "menu.help": "&Help",
    "file.new": "New", "file.open": "Open…", "file.save": "Save", "file.save_as": "Save As…", "file.import": "Import…", "file.export": "Export…", "file.export_drawing": "Export drawing (SVG)…", "file.recent": "Open Recent", "file.quit": "Quit",
    "edit.undo": "Undo", "edit.redo": "Redo", "edit.delete": "Delete", "edit.copy": "Copy with Placement", "edit.paste": "Paste with Placement", "edit.select_all": "Select All", "edit.invert": "Invert Selection", "edit.select_same_material": "Select Same Material", "edit.preferences": "Preferences…",
    "view.fit": "Fit All", "view.focus": "Focus Selection", "view.front": "Front", "view.top": "Top", "view.right": "Right", "view.iso": "Isometric", "view.ortho": "Orthographic", "view.grid": "Grid", "view.mode": "Display Mode", "view.isolate": "Isolate", "view.show_all": "Show All", "view.hide": "Hide", "view.section": "Section Analysis", "view.build_plate": "Build Plate Preview", "view.high_contrast": "High-Contrast Theme",
    "palette.placeholder": "Type a command… (Ctrl+Space)", "palette.conflict": "conflicts with",
    "numeric.hint": "Tab: type an exact value  •  Enter: confirm  •  Esc: cancel",
    "status.ready": "Ready", "status.validated": "Validated: watertight",
    "dialog.units": "Units of the file", "dialog.units.text": "This format carries no unit. What are the numbers in?",
    "outliner.search": "Search (Ctrl+F)…", "outliner.title": "Outliner", "properties.title": "Selection", "materials.title": "Materials", "measure.copied": "Measurement copied",
    "export.blocked": "Export blocked", "export.done": "Exported",
}

_table: dict[str, str] = dict(_EN)
_lang = os.environ.get("ROBOCAD_LANG", "en")
if _lang != "en":
    p = os.path.join(os.path.dirname(__file__), f"strings.{_lang}.json")
    if os.path.exists(p):
        with open(p, encoding="utf-8") as f:
            _table.update(json.load(f))


def tr(key: str, default: str | None = None) -> str:
    return _table.get(key, default if default is not None else key)
