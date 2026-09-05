"""Widgets: the command palette, the numeric-entry bar, the outliner,
the selection/properties panel with live dimensions, the materials
panel, radial menus, the disambiguation menu, and export/units dialogs."""

from __future__ import annotations

import json
import math
import os
from typing import Callable, Optional

from PySide6.QtCore import QEvent, QPoint, QPointF, QRect, QSize, Qt, Signal, QProcess, QTimer
from PySide6.QtGui import QAction, QColor, QDrag, QFont, QKeySequence, QPainter, QPen, QBrush
from PySide6.QtWidgets import (
    QAbstractItemView,
    QApplication,
    QCheckBox,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QDoubleSpinBox,
    QFileDialog,
    QFormLayout,
    QFrame,
    QGridLayout,
    QHeaderView,
    QHBoxLayout,
    QInputDialog,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMenu,
    QMessageBox,
    QPushButton,
    QSpinBox,
    QStyle,
    QTreeWidget,
    QTreeWidgetItem,
    QVBoxLayout,
    QWidget,
)

from ..document import Document, Material
from ..units import ExpressionError, evaluate, format_angle, format_length
from .strings import tr


# ------------------------------------------------------------ command palette


class CommandPalette(QDialog):
    """Ctrl+Space: a searchable list of every command with shortcut hints
    and conflict warnings."""

    def __init__(self, commands: dict[str, dict], parent=None):
        super().__init__(parent, Qt.Popup | Qt.FramelessWindowHint)
        self.commands = commands
        self.setMinimumWidth(520)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(8, 8, 8, 8)
        self.edit = QLineEdit()
        self.edit.setPlaceholderText(tr("palette.placeholder"))
        self.list = QListWidget()
        lay.addWidget(self.edit)
        lay.addWidget(self.list)
        self.edit.textChanged.connect(self.refresh)
        self.list.itemActivated.connect(self._run)
        self.edit.installEventFilter(self)
        self.refresh("")

    def conflicts(self) -> dict[str, list[str]]:
        by_key: dict[str, list[str]] = {}
        for cid, c in self.commands.items():
            for k in c.get("keys", []):
                by_key.setdefault(k.lower(), []).append(cid)
        return {k: v for k, v in by_key.items() if len(v) > 1}

    def refresh(self, text: str):
        self.list.clear()
        t = text.lower().strip()
        conflicts = self.conflicts()
        scored = []
        for cid, c in self.commands.items():
            label = c["label"]
            hay = f"{cid} {label} {c.get('category', '')}".lower()
            if not t:
                score = 0
            elif t in label.lower():
                score = 1 + label.lower().index(t)
            elif all(ch in hay for ch in t):
                score = 50
            else:
                continue
            scored.append((score, cid, c))
        scored.sort(key=lambda s: (s[0], s[2]["label"]))
        for _, cid, c in scored[:60]:
            keys = ", ".join(c.get("keys", []))
            warn = ""
            for k in c.get("keys", []):
                if k.lower() in conflicts:
                    others = [self.commands[o]["label"] for o in conflicts[k.lower()] if o != cid]
                    warn = f"  ⚠ {tr('palette.conflict')} {', '.join(others)}"
            item = QListWidgetItem(f"{c.get('category', '')}: {c['label']}" + (f"    [{keys}]" if keys else "") + warn)
            item.setData(Qt.UserRole, cid)
            self.list.addItem(item)
        if self.list.count():
            self.list.setCurrentRow(0)

    def eventFilter(self, obj, ev):
        if obj is self.edit and ev.type() == QEvent.KeyPress:
            if ev.key() in (Qt.Key_Down, Qt.Key_Up):
                row = self.list.currentRow() + (1 if ev.key() == Qt.Key_Down else -1)
                self.list.setCurrentRow(max(0, min(self.list.count() - 1, row)))
                return True
            if ev.key() in (Qt.Key_Return, Qt.Key_Enter):
                item = self.list.currentItem()
                if item:
                    self._run(item)
                return True
            if ev.key() == Qt.Key_Escape:
                self.close()
                return True
        return super().eventFilter(obj, ev)

    def _run(self, item):
        cid = item.data(Qt.UserRole)
        self.close()
        self.commands[cid]["run"]()

    def open_at(self, center: QPoint):
        self.edit.clear()
        self.refresh("")
        self.move(center - QPoint(self.width() // 2, 0))
        self.show()
        self.edit.setFocus()


# ------------------------------------------------------------- numeric entry


class NumericBar(QWidget):
    """The Tab-to-type bar: one field per dimension, unit-aware expressions.
    Enter commits, Escape cancels, Tab cycles fields."""

    committed = Signal(list)
    cancelled = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.lay = QHBoxLayout(self)
        self.lay.setContentsMargins(6, 2, 6, 2)
        self.fields = []
        self.edits: list[QLineEdit] = []
        self.hint = QLabel(tr("numeric.hint"))
        self.hint.setStyleSheet("color: #8a8f99;")
        self.setVisible(False)

    def set_fields(self, fields):
        for i in reversed(range(self.lay.count())):
            w = self.lay.itemAt(i).widget()
            if w:
                w.setParent(None)
        self.fields = fields
        self.edits = []
        for f in fields:
            self.lay.addWidget(QLabel(f.name))
            e = QLineEdit(format_angle(f.value) if f.angle else (f"{f.value:g}" if not f.unit else format_length(f.value)))
            e.setFixedWidth(110)
            e.installEventFilter(self)
            e.textChanged.connect(self._validate)
            self.lay.addWidget(e)
            self.edits.append(e)
        self.lay.addWidget(self.hint)
        self.lay.addStretch(1)
        self.setVisible(bool(fields))

    def focus_first(self):
        if self.edits:
            self.edits[0].setFocus()
            self.edits[0].selectAll()

    def values(self) -> Optional[list[float]]:
        out = []
        for f, e in zip(self.fields, self.edits):
            try:
                out.append(evaluate(e.text(), angle=f.angle, default_unit=None if f.angle or not f.unit else "mm"))
            except ExpressionError:
                return None
        return out

    def _validate(self):
        for f, e in zip(self.fields, self.edits):
            try:
                evaluate(e.text(), angle=f.angle, default_unit=None if f.angle or not f.unit else "mm")
                e.setStyleSheet("")
            except ExpressionError:
                e.setStyleSheet("border: 1px solid #d05050;")

    def eventFilter(self, obj, ev):
        if ev.type() == QEvent.KeyPress and obj in self.edits:
            if ev.key() in (Qt.Key_Return, Qt.Key_Enter):
                vals = self.values()
                if vals is not None:
                    self.committed.emit(vals)
                return True
            if ev.key() == Qt.Key_Escape:
                self.cancelled.emit()
                return True
            if ev.key() == Qt.Key_Tab:
                i = self.edits.index(obj)
                nxt = self.edits[(i + 1) % len(self.edits)]
                nxt.setFocus()
                nxt.selectAll()
                return True
        return super().eventFilter(obj, ev)


# ----------------------------------------------------------------- outliner


class Outliner(QWidget):
    """Hierarchical groups, drag-and-drop, search, show/hide/lock/disable,
    isolate, and the active group."""

    def __init__(self, app, parent=None):
        super().__init__(parent)
        self.app = app
        lay = QVBoxLayout(self)
        lay.setContentsMargins(4, 4, 4, 4)
        self.search = QLineEdit()
        self.search.setPlaceholderText(tr("outliner.search"))
        self.search.textChanged.connect(self.refresh)
        lay.addWidget(self.search)
        actions = QHBoxLayout()
        for label, callback in (("New group", lambda: self._group([])),
                                ("Expand all", lambda: self.tree.expandAll()),
                                ("Collapse all", lambda: self.tree.collapseAll())):
            button = QPushButton(label)
            button.clicked.connect(callback)
            actions.addWidget(button)
        lay.addLayout(actions)
        self.tree = QTreeWidget()
        self.tree.setHeaderLabels(["Name", "", "", ""])
        self.tree.setColumnWidth(0, 200)
        self.tree.header().setStretchLastSection(False)
        self.tree.header().setSectionResizeMode(0, QHeaderView.Stretch)
        for c in (1, 2, 3):
            self.tree.setColumnWidth(c, 22)
        self.tree.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.tree.setDragDropMode(QAbstractItemView.InternalMove)
        self.tree.setDefaultDropAction(Qt.MoveAction)
        self.tree.setToolTip("Right-click to fit a part or group in view. Drag parts into groups to organize them.")
        self.tree.itemSelectionChanged.connect(self._select)
        self.tree.itemChanged.connect(self._renamed)
        self.tree.itemDoubleClicked.connect(self._double)
        self.tree.itemClicked.connect(self._clicked)
        self.tree.setContextMenuPolicy(Qt.CustomContextMenu)
        self.tree.customContextMenuRequested.connect(self._menu)
        self.tree.dropEvent = self._drop
        self.tree.itemExpanded.connect(lambda item: self._expanded(item, True))
        self.tree.itemCollapsed.connect(lambda item: self._expanded(item, False))
        lay.addWidget(self.tree)
        self._updating = False
        self._expansion = {}

    def _expanded(self, item, expanded):
        if not self._updating and not self.search.text().strip():
            self._expansion[item.data(0, Qt.UserRole)] = expanded

    def refresh(self, *_):
        self._updating = True
        scroll = self.tree.verticalScrollBar().value()
        self.tree.clear()
        doc = self.app.doc
        text = self.search.text().lower().strip()
        selected = set(self.app.viewport.selection.nodes())
        matches = set()
        if text:
            for n in doc.nodes.values():
                if text in n.name.lower():
                    matches.add(n.id)
                    matches.update(c.id for c in doc.walk(n.id))
                    p = n.parent
                    while p:
                        matches.add(p)
                        p = doc.nodes[p].parent

        def add(parent_item, node_id):
            n = doc.nodes[node_id]
            if text and node_id not in matches:
                return
            it = QTreeWidgetItem([n.name, "👁" if n.visible else "◌", "🔒" if n.locked else "", "⏸" if n.disabled else ""])
            it.setIcon(0, self.style().standardIcon(QStyle.SP_DirIcon if n.kind == 'group' else QStyle.SP_FileIcon))
            it.setToolTip(0, f"{n.name}\n{n.kind.capitalize()} · Right-click for Fit in view and organization")
            it.setData(0, Qt.UserRole, node_id)
            flags = it.flags() | Qt.ItemIsEditable | Qt.ItemIsDragEnabled
            it.setFlags(flags | Qt.ItemIsDropEnabled if n.kind == "group" else flags & ~Qt.ItemIsDropEnabled)
            if node_id == doc.active_group:
                it.setForeground(0, QColor(120, 200, 255))
            if not doc.is_visible(node_id):
                it.setForeground(0, QColor(130, 130, 135))
            (parent_item.addChild if parent_item else self.tree.addTopLevelItem)(it)
            for c in n.children:
                add(it, c)
            it.setExpanded(bool(text) or self._expansion.get(node_id, True))
            if node_id in selected:
                it.setSelected(True)

        for r in doc.roots:
            if r in doc.nodes:
                add(None, r)
        self._updating = False
        self.tree.verticalScrollBar().setValue(scroll)

    def sync_selection(self):
        """Update existing tree items; selection does not change the tree."""
        from PySide6.QtWidgets import QTreeWidgetItemIterator
        self._updating=True
        self.tree.blockSignals(True)
        try:
            selected=set(self.app.viewport.selection.nodes())
            iterator=QTreeWidgetItemIterator(self.tree)
            while iterator.value():
                item=iterator.value();want=item.data(0,Qt.UserRole) in selected
                if item.isSelected()!=want:item.setSelected(want)
                iterator+=1
        finally:
            self.tree.blockSignals(False);self._updating=False

    def _select(self):
        if self._updating:
            return
        ids = [it.data(0, Qt.UserRole) for it in self.tree.selectedItems()]
        sel = self.app.viewport.selection
        sel.clear()
        for i in ids:
            sel.items.append((i, "body", 0))
        self.app.selection_changed(None, from_outliner=True)

    def _renamed(self, item, column):
        if self._updating or column != 0:
            return
        nid = item.data(0, Qt.UserRole)
        text = item.text(0)
        name = text.strip()
        if nid in self.app.doc.nodes and self.app.doc.nodes[nid].name != name:
            self.app.ops.rename(nid, name)

    def _double(self, item, column):
        if column == 0:
            self.tree.editItem(item, 0)

    def _clicked(self, item, column):
        nid = item.data(0, Qt.UserRole)
        n = self.app.doc.nodes.get(nid)
        if n is None:
            return
        if column == 1:
            self.app.ops.set_visible([nid], not n.visible)
        elif column == 2:
            self.app.ops.set_locked([nid], not n.locked)
        elif column == 3:
            self.app.ops.set_disabled([nid], not n.disabled)

    def _drop(self, event):
        target = self.tree.itemAt(event.position().toPoint())
        moving = [it.data(0, Qt.UserRole) for it in self.tree.selectedItems()]
        parent = None
        index = None
        if target is not None:
            tid = target.data(0, Qt.UserRole)
            tn = self.app.doc.nodes[tid]
            if tn.kind == "group":
                parent = tid
            else:
                parent = tn.parent
                index = self.app.doc.index_of(tid)
        self.app._safe(lambda: self.app.ops.move_nodes(moving, parent, index))
        event.accept()
        self.refresh()

    def _menu(self, pos):
        target = self.tree.itemAt(pos)
        if target is not None and not target.isSelected():
            self.tree.clearSelection()
            target.setSelected(True)
            self.tree.setCurrentItem(target)
        menu = self._context_menu()
        menu.exec(self.tree.viewport().mapToGlobal(pos))

    def _group(self, ids):
        name, ok = QInputDialog.getText(self, "Organize components", "Group name:")
        if ok and name.strip():
            self.app._safe(lambda: self.app.ops.group(ids, name.strip()))

    def _fit(self, ids):
        if self.app.viewport.focus_nodes(ids):
            self.app.status("Fit in view: " + ', '.join(self.app.doc.nodes[i].name for i in ids))
        else:
            self.app.status("No geometry to frame in this selection")

    def _context_menu(self):
        items = self.tree.selectedItems()
        ids = [it.data(0, Qt.UserRole) for it in items]
        menu = QMenu(self)
        if ids:
            menu.addAction("Fit in view", lambda: self._fit(ids))
            menu.addSeparator()
            menu.addAction("Isolate", lambda: self.app.ops.isolate(ids))
            menu.addAction("Hide", lambda: self.app.ops.set_visible(ids, False))
            menu.addAction("Show", lambda: self.app.ops.set_visible(ids, True))
            menu.addAction("Lock", lambda: self.app.ops.set_locked(ids, True))
            menu.addAction("Unlock", lambda: self.app.ops.set_locked(ids, False))
            menu.addAction("Group selection…", lambda: self._group(ids))
            move = QMenu("Move to group", menu)
            menu.addMenu(move)
            move.addAction("Top level", lambda: self.app._safe(lambda: self.app.ops.move_nodes(ids, None)))
            excluded = set(ids)
            for nid in ids:
                excluded.update(n.id for n in self.app.doc.walk(nid))
            for node in self.app.doc.walk():
                if node.kind != 'group' or node.id in excluded:
                    continue
                path = [node.name]
                p = node.parent
                while p:
                    path.insert(0, self.app.doc.nodes[p].name)
                    p = self.app.doc.nodes[p].parent
                move.addAction(' / '.join(path), lambda checked=False, nid=node.id: self.app._safe(lambda: self.app.ops.move_nodes(ids, nid)))
            menu.addAction("Make unique (bake instance)", lambda: [self.app.ops.make_unique(i) for i in ids if self.app.doc.nodes[i].kind == "instance"])
            if len(ids) == 1 and self.app.doc.nodes[ids[0]].kind == "group":
                menu.addAction("Set as active group", lambda: self.app.ops.set_active_group(ids[0]))
            menu.addAction("Delete", lambda: self.app.ops.delete(ids))
        menu.addAction("Clear active group", lambda: self.app.ops.set_active_group(None))
        menu.addAction("Show all", self.app.ops.show_all)
        return menu


# ------------------------------------------------------- properties panel


class PropertiesPanel(QWidget):
    """Selection facts (bounding box, volume, area, mass) and the live
    dimensions of the selected faces/edges, editable in place."""

    def __init__(self, app, parent=None):
        super().__init__(parent)
        self.app = app
        self.lay = QVBoxLayout(self)
        self.lay.setContentsMargins(6, 6, 6, 6)
        self.facts = QLabel("—")
        self.facts.setWordWrap(True)
        self.facts.setTextInteractionFlags(Qt.TextSelectableByMouse)
        self.lay.addWidget(self.facts)
        self.measure = QPushButton("Calculate exact measurements")
        self.measure.setToolTip("Calculate volume, area and mass in a separate process. Select a face to edit its dimensions.")
        self.measure.clicked.connect(self._measure_selection)
        self.lay.addWidget(self.measure)
        self._measurement_process = None
        self._measurement_key = None
        self._measurement_result = None
        self.dims = QFormLayout()
        self.lay.addLayout(self.dims)
        self.material = QComboBox()
        self.material.currentIndexChanged.connect(self._material_changed)
        self.lay.addWidget(QLabel("Material"))
        self.lay.addWidget(self.material)
        self.tol = QDoubleSpinBox()
        self.tol.setRange(0.005, 2.0)
        self.tol.setSingleStep(0.01)
        self.tol.setDecimals(3)
        self.tol.setValue(0.05)
        self.tol.valueChanged.connect(self._tol_changed)
        self.lay.addWidget(QLabel("Tessellation tolerance (mm)"))
        self.lay.addWidget(self.tol)
        self.lay.addStretch(1)
        self._updating = False

    def refresh(self):
        self._updating = True
        doc = self.app.doc
        ids = [i for i in self.app.viewport.selection.nodes() if i in doc.nodes]
        key = (id(doc), doc.revision, tuple(ids))
        if key != self._measurement_key:
            self.cancel_measurement()
            self._measurement_key = key
            self._measurement_result = None
        while self.dims.rowCount():
            self.dims.removeRow(0)
        self.material.clear()
        for m in doc.materials.values():
            self.material.addItem(f"{m.name}  ({m.density:g} g/cm³)", m.id)
        if not ids:
            self.facts.setText("Nothing selected.")
            self.measure.setEnabled(False)
            self._updating = False
            return
        self.measure.setEnabled(self._measurement_process is None and any(doc.nodes[i].kind in ('body', 'sheet', 'instance') for i in ids))
        self.facts.setText(self._measurement_result or self._selection_preview(ids))
        n = doc.nodes[ids[0]]
        if n.material:
            self.material.setCurrentIndex(max(0, self.material.findData(n.material)))
        self.tol.setValue(n.tessellation_tolerance)
        # Live dimensions from the selection.
        for label, value, angle, setter in self.app.live_dimensions():
            edit = QLineEdit(format_angle(value) if angle else format_length(value))
            edit.editingFinished.connect(lambda e=edit, s=setter, a=angle: self._commit(e, s, a))
            self.dims.addRow(label, edit)
        # Joint physics: inferred values with overrides.
        if n.kind == "joint" and n.joint is not None:
            phys = self._joint_physics(n)
            over = (n.robot or {}).get("physics", {})
            for key, label, scale in (("clearance", "Clearance (mm)", 1e3), ("backlash", "Backlash (°)", 180 / math.pi), ("wobble", "Wobble (°)", 180 / math.pi)):
                v = over.get(key, phys.get(key, 0.0))
                edit = QLineEdit(f"{v * scale:.4g}")
                edit.editingFinished.connect(lambda e=edit, k=key, sc=scale, jid=n.id: self._joint_override(jid, {k: float(evaluate(e.text())) / sc}))
                self.dims.addRow(label + (" *" if key in over else ""), edit)
            for key, label in (("coulomb", "Coulomb friction (mN·m)"), ("viscous", "Viscous (mN·m·s)")):
                v = over.get("friction", {}).get(key, phys.get("friction", {}).get(key, 0.0))
                edit = QLineEdit(f"{v * 1e3:.4g}")
                edit.editingFinished.connect(lambda e=edit, k=key, jid=n.id: self._joint_override(jid, {"friction": {k: float(evaluate(e.text())) * 1e-3}}))
                self.dims.addRow(label + (" *" if key in over.get("friction", {}) else ""), edit)
            v = over.get("stiffness", {}).get("radial", phys.get("stiffness", {}).get("radial", 0.0))
            edit = QLineEdit(f"{v:.4g}")
            edit.editingFinished.connect(lambda e=edit, jid=n.id: self._joint_override(jid, {"stiffness": {"radial": float(evaluate(e.text()))}}))
            self.dims.addRow("Radial stiffness (N/m)" + (" *" if "radial" in over.get("stiffness", {}) else ""), edit)
            patch = QLineEdit(f"{phys.get('flex_patch_radius', .004)*1e3:.4g}")
            patch.setObjectName('flex_patch_radius')
            patch.setToolTip('Radius of the rigid flex attachment patch, in mm. A larger patch changes the boundary condition, not just mesh quality. Inferred: hole radius + 2.4 mm wall, at least 4 mm. Blank restores inference.')
            patch.editingFinished.connect(lambda e=patch, jid=n.id: self._flex_patch_override(jid, e.text()))
            self.dims.addRow('Flex patch radius (mm)' + (' *' if over.get('flex_patch_radius') is not None else ''), patch)
            self.dims.addRow(QLabel(f"source: {phys.get('source', '?')}, pin Ø{phys.get('pin_radius', 0) * 2e3:.2f} mm in Ø{phys.get('hole_radius', 0) * 2e3:.2f} mm over {phys.get('contact_length', 0) * 1e3:.1f} mm; * = overridden"))
        if n.results:
            r = n.results
            keys = [k for k in ("peak_stress_pa", "yield_margin", "max_deflection_m", "peak_temperature_c", "peak_reaction_force_n", "bearing_margin", "peak_current_a", "stall_margin", "peak_winding_c") if r.get(k) is not None]
            if keys:
                self.dims.addRow(QLabel("Results: " + ", ".join(f"{k} {r[k]:.3g}" for k in keys)))
        b = QPushButton("Material properties…")
        b.clicked.connect(lambda: self._edit_material(n.material))
        self.dims.addRow(b)
        self._updating = False

    def _selection_preview(self, ids):
        # Reuse display bounds: a click must never integrate B-reps, enumerate
        # face properties, or trigger tessellation of an undisplayed part.
        items = self.app.viewport.items
        bounds = [items[i].bbox for i in ids if i in items]
        text = f"{len(ids)} item(s)"
        if bounds:
            size = [max(b[1][j] for b in bounds) - min(b[0][j] for b in bounds) for j in range(3)]
            text += f"\nDisplay size ≈ {size[0]:.3f} × {size[1]:.3f} × {size[2]:.3f} mm"
        return text + "\nExact measurements available on request."

    def cancel_measurement(self):
        process = self._measurement_process
        self._measurement_process = None
        if process is not None:
            process.kill()

    def _measure_selection(self):
        import sys
        import tempfile
        from pathlib import Path
        from PySide6.QtCore import QProcessEnvironment

        self.cancel_measurement()
        doc = self.app.doc
        ids = [i for i in self.app.viewport.selection.nodes() if i in doc.nodes]
        key = (id(doc), doc.revision, tuple(ids))
        folder = tempfile.TemporaryDirectory(prefix='robocad-measure-')
        root = Path(folder.name)
        try:
            entries = []
            for i in ids:
                body = doc.resolved_body(i)
                if body is None:
                    continue
                filename = f'{len(entries)}.brep'
                (root / filename).write_bytes(doc.kernel.serialize(body))
                entries.append({'file': filename, 'kind': body.kind, 'density': doc.density_of(i)})
            (root / 'input.json').write_text(json.dumps(entries))
        except Exception as error:
            folder.cleanup()
            self.app.status(f'Measurements failed: {error}')
            return
        process = QProcess(self)
        env = QProcessEnvironment.systemEnvironment()
        env.insert('PYTHONPATH', str(Path(__file__).resolve().parents[2]) + os.pathsep + env.value('PYTHONPATH'))
        process.setProcessEnvironment(env)
        self._measurement_process = process
        self.measure.setEnabled(False)
        self.facts.setText('Calculating exact measurements… You can keep working.')
        timer = QTimer(process)
        timer.setSingleShot(True)
        timer.timeout.connect(process.kill)

        def finished(*_):
            timer.stop()
            if self._measurement_process is process:
                self._measurement_process = None
                current = (id(self.app.doc), self.app.doc.revision, tuple(self.app.viewport.selection.nodes()))
                if current == key:
                    try:
                        if process.exitStatus() != QProcess.NormalExit or process.exitCode() != 0:
                            raise ValueError('calculation stopped or failed; try a smaller selection')
                        result = json.loads(bytes(process.readAllStandardOutput()).decode())
                        s = result['size']; c = result['centroid']
                        self._measurement_result = (f"{len(entries)} measured item(s)\nsize {s[0]:.3f} × {s[1]:.3f} × {s[2]:.3f} mm"
                            f"\nvolume {result['volume'] / 1000:.3f} cm³\narea {result['area'] / 100:.2f} cm²"
                            f"\nmass {result['mass_g']:.2f} g\ncentroid ({c[0]:.2f}, {c[1]:.2f}, {c[2]:.2f})")
                        self.facts.setText(self._measurement_result)
                    except Exception as error:
                        self.facts.setText(f'Measurements unavailable: {error}')
                    self.measure.setEnabled(True)
            folder.cleanup()
            process.deleteLater()

        process.finished.connect(finished)
        process.errorOccurred.connect(lambda error: finished() if error == QProcess.FailedToStart else None)
        process.start(sys.executable, ['-m', 'robocad.measure_worker', str(root)])
        timer.start(60000)

    def _joint_physics(self, n) -> dict:
        """Inspect this joint without building a simulation's collision data."""
        try:
            from ..physical import inspect_joint_physics
            doc = self.app.doc
            # Large imported B-reps must not trigger whole-assembly mass/face
            # integration when a connector is clicked. Use editable declared
            # values; exact derivation remains in the offline export/REST call.
            display_indices=sum(item.indices.size for item in self.app.viewport.items.values())
            if display_indices > 150000:
                result=dict((n.robot or {}).get('physics') or {})
                result['source']='declared overrides; geometric inference deferred for this large assembly'
                if result.get('flex_patch_radius') is None:result['flex_patch_radius']=.004
                return result
            if getattr(self, '_joint_revision', None) != doc.revision:
                self._joint_revision = doc.revision
                self._joint_cache = {}
            if n.id not in self._joint_cache:
                self._joint_cache[n.id] = inspect_joint_physics(doc, n.id)
            return self._joint_cache[n.id]
        except Exception as error:
            self.app.status(f'Joint inspection failed: {error}')
            return {}

    def _joint_override(self, jid, fields):
        try:
            self.app.ops.set_joint_physics(jid, **fields)
        except Exception as e:
            self.app.error(str(e))

    def _flex_patch_override(self, jid, text):
        try:
            radius = evaluate(text, default_unit='mm') * 1e-3 if text.strip() else None
            self.app.ops.set_joint_physics(jid, flex_patch_radius=radius)
        except Exception as e:
            self.app.error(str(e))

    def _edit_material(self, mid):
        if mid not in self.app.doc.materials:
            return
        m = self.app.doc.materials[mid]
        props = m.props()
        d = QDialog(self)
        d.setWindowTitle(f"{m.name}: engineering properties")
        form = QFormLayout(d)
        edits = {}
        for key, label, scale in (("youngs_modulus", "Young's modulus (GPa)", 1e-9), ("poisson", "Poisson ratio", 1.0), ("yield_strength", "Yield strength (MPa)", 1e-6), ("ultimate_strength", "Ultimate strength (MPa)", 1e-6), ("glass_transition_c", "Glass transition (°C)", 1.0), ("thermal_conductivity", "Thermal conductivity (W/m·K)", 1.0), ("specific_heat", "Specific heat (J/kg·K)", 1.0), ("thermal_expansion", "Thermal expansion (1/K)", 1.0), ("bearing_pressure", "Allowable bearing pressure (MPa)", 1e-6)):
            e = QLineEdit(f"{props[key] * scale:.4g}")
            edits[key] = (e, scale)
            form.addRow(label, e)
        fr = QLineEdit(f"{props['friction']['self']['kinetic']:.3g}")
        fs = QLineEdit(f"{props['friction'].get('steel', props['friction']['self'])['kinetic']:.3g}")
        form.addRow("Kinetic friction vs itself", fr)
        form.addRow("Kinetic friction vs steel", fs)
        aniso = QLineEdit(f"{(props.get('print') or {}).get('anisotropy_z', 1.0):.3g}")
        form.addRow("Print anisotropy across layers (E ratio)", aniso)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(d.accept)
        bb.rejected.connect(d.reject)
        form.addRow(bb)
        if d.exec():
            try:
                values = {k: float(evaluate(e.text())) / sc for k, (e, sc) in edits.items()}
                mu_self, mu_steel = float(evaluate(fr.text())), float(evaluate(fs.text()))
                values["friction"] = {"self": {"static": mu_self * 1.2, "kinetic": mu_self}, "world": {"static": mu_self * 1.2, "kinetic": mu_self}, "steel": {"static": mu_steel * 1.2, "kinetic": mu_steel}}
                if props.get("print") is not None:
                    values["print"] = {**props["print"], "anisotropy_z": float(evaluate(aniso.text()))}
                self.app.ops.set_material_props(mid, **values)
            except Exception as e:
                self.app.error(str(e))

    def _commit(self, edit, setter, angle):
        try:
            v = evaluate(edit.text(), angle=angle, default_unit=None if angle else "mm")
        except ExpressionError as e:
            self.app.error(str(e))
            return
        setter(v)

    def _material_changed(self, idx):
        if self._updating or idx < 0:
            return
        ids = self.app.viewport.selection.nodes()
        if ids:
            self.app.ops.set_material(ids, self.material.itemData(idx))

    def _tol_changed(self, v):
        if self._updating:
            return
        for i in self.app.viewport.selection.nodes():
            self.app.doc.nodes[i].tessellation_tolerance = v
            self.app.doc.touch(i)


# ----------------------------------------------------------- materials panel


class MaterialsPanel(QWidget):
    def __init__(self, app, parent=None):
        super().__init__(parent)
        self.app = app
        lay = QVBoxLayout(self)
        lay.setContentsMargins(4, 4, 4, 4)
        self.search = QLineEdit()
        self.search.setPlaceholderText("Search materials…")
        self.search.textChanged.connect(self.refresh)
        lay.addWidget(self.search)
        self.list = QListWidget()
        self.list.setDragEnabled(True)
        self.list.itemDoubleClicked.connect(self._apply)
        lay.addWidget(self.list)
        row = QHBoxLayout()
        b = QPushButton("Apply to selection")
        b.clicked.connect(lambda: self._apply(self.list.currentItem()))
        row.addWidget(b)
        n = QPushButton("New…")
        n.clicked.connect(self._new)
        row.addWidget(n)
        lay.addLayout(row)
        self.list.startDrag = self._start_drag

    def refresh(self, *_):
        self.list.clear()
        t = self.search.text().lower()
        for m in self.app.doc.materials.values():
            if t and t not in m.name.lower() and not any(t in tag for tag in m.tags):
                continue
            it = QListWidgetItem(f"■ {m.name}   {m.density:g} g/cm³")
            it.setForeground(QColor(int(m.color[0] * 255), int(m.color[1] * 255), int(m.color[2] * 255)))
            it.setData(Qt.UserRole, m.id)
            self.list.addItem(it)

    def _apply(self, item):
        if item is None:
            return
        ids = self.app.viewport.selection.nodes()
        if ids:
            self.app.ops.set_material(ids, item.data(Qt.UserRole))

    def _start_drag(self, actions):
        item = self.list.currentItem()
        if item is None:
            return
        from PySide6.QtCore import QMimeData

        mime = QMimeData()
        mime.setText(f"material:{item.data(Qt.UserRole)}")
        drag = QDrag(self.list)
        drag.setMimeData(mime)
        drag.exec(Qt.CopyAction)

    def _new(self):
        d = QDialog(self)
        d.setWindowTitle("New material")
        form = QFormLayout(d)
        name = QLineEdit("Custom")
        dens = QDoubleSpinBox()
        dens.setRange(0.01, 25.0)
        dens.setValue(1.2)
        dens.setDecimals(3)
        form.addRow("Name", name)
        form.addRow("Density (g/cm³)", dens)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(d.accept)
        bb.rejected.connect(d.reject)
        form.addRow(bb)
        if d.exec():
            mid = name.text().lower().replace(" ", "_") or "custom"
            from ..commands import SetMaterialDef

            self.app.ops.stack.push(SetMaterialDef("New material", Material(mid, name.text(), dens.value())))
            self.refresh()


# --------------------------------------------------------------- radial menu


class RadialMenu(QWidget):
    """A pie menu: entries around the cursor; release/click on one runs it."""

    def __init__(self, entries: list[tuple[str, Callable]], parent=None):
        super().__init__(parent, Qt.Popup | Qt.FramelessWindowHint | Qt.NoDropShadowWindowHint)
        self.setAttribute(Qt.WA_TranslucentBackground)
        self.entries = entries
        self.radius = 78
        self.setFixedSize(2 * self.radius + 120, 2 * self.radius + 120)
        self.hover_index = -1
        self.setMouseTracking(True)

    def open_at(self, global_pos: QPoint):
        self.move(global_pos - QPoint(self.width() // 2, self.height() // 2))
        self.show()

    def _index_at(self, pos) -> int:
        c = QPointF(self.width() / 2, self.height() / 2)
        dx, dy = pos.x() - c.x(), pos.y() - c.y()
        if math.hypot(dx, dy) < 18:
            return -1
        ang = (math.degrees(math.atan2(dy, dx)) + 90 + 360) % 360
        n = len(self.entries)
        return int((ang + 180 / n) // (360 / n)) % n

    def mouseMoveEvent(self, e):
        i = self._index_at(e.position())
        if i != self.hover_index:
            self.hover_index = i
            self.update()

    def mouseReleaseEvent(self, e):
        i = self._index_at(e.position())
        self.close()
        if i >= 0:
            self.entries[i][1]()

    def mousePressEvent(self, e):
        self.mouseReleaseEvent(e)

    def keyPressEvent(self, e):
        if e.key() == Qt.Key_Escape:
            self.close()

    def paintEvent(self, e):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        c = QPointF(self.width() / 2, self.height() / 2)
        n = len(self.entries)
        p.setPen(Qt.NoPen)
        for i, (label, _) in enumerate(self.entries):
            ang = math.radians(i * 360 / n - 90)
            x, y = c.x() + self.radius * math.cos(ang), c.y() + self.radius * math.sin(ang)
            active = i == self.hover_index
            p.setBrush(QBrush(QColor(90, 150, 255, 230) if active else QColor(40, 42, 48, 220)))
            p.drawEllipse(QPointF(x, y), 46, 22)
            p.setPen(QPen(QColor(255, 255, 255)))
            p.setFont(QFont("Helvetica", 9, QFont.Bold if active else QFont.Normal))
            p.drawText(QRect(int(x - 46), int(y - 22), 92, 44), Qt.AlignCenter, label)
            p.setPen(Qt.NoPen)
        p.setBrush(QBrush(QColor(255, 255, 255, 60)))
        p.drawEllipse(c, 6, 6)


# ----------------------------------------------------------------- dialogs


def disambiguation_menu(parent, candidates, doc: Document, pos_global: QPoint, on_pick: Callable):
    menu = QMenu(parent)
    for kind, nid, idx in candidates:
        n = doc.nodes.get(nid)
        label = f"{n.name if n else nid}: {kind}" + (f" #{idx}" if kind != "body" else "")
        menu.addAction(label, lambda c=(kind, nid, idx): on_pick(c))
    menu.exec(pos_global)


class UnitsDialog(QDialog):
    def __init__(self, guess: str, parent=None):
        super().__init__(parent)
        self.setWindowTitle(tr("dialog.units"))
        lay = QVBoxLayout(self)
        lay.addWidget(QLabel(tr("dialog.units.text")))
        self.combo = QComboBox()
        for u in ("mm", "cm", "m", "in", "ft"):
            self.combo.addItem(u)
        self.combo.setCurrentText(guess)
        lay.addWidget(self.combo)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        lay.addWidget(bb)

    def unit(self) -> str:
        return self.combo.currentText()


class ExportDialog(QDialog):
    """Format-specific options; remembers the last values through `settings`."""

    def __init__(self, fmt: str, settings: dict, parent=None):
        super().__init__(parent)
        self.setWindowTitle(f"Export {fmt.upper()}")
        self.fmt = fmt
        self.settings = settings
        form = QFormLayout(self)
        self.widgets = {}
        if fmt == "stl":
            self._combo(form, "binary", "Format", ["binary", "ascii"], "binary" if settings.get("binary", True) else "ascii")
            self._combo(form, "unit", "Unit", ["mm", "cm", "m", "in", "ft"], settings.get("unit", "mm"))
            self._spin(form, "tolerance", "Chord tolerance (mm)", settings.get("tolerance", 0.05), 0.001, 1.0, 3)
            self._spin(form, "angular_deg", "Angular tolerance (°)", settings.get("angular_deg", 20.0), 1.0, 60.0, 1)
        elif fmt == "3mf":
            self._spin(form, "tolerance", "Chord tolerance (mm)", settings.get("tolerance", 0.05), 0.001, 1.0, 3)
            self._check(form, "colors", "Write colours", settings.get("colors", True))
            self._check(form, "names", "Write names", settings.get("names", True))
        elif fmt == "obj":
            self._spin(form, "tolerance", "Chord tolerance (mm)", settings.get("tolerance", 0.05), 0.001, 1.0, 3)
            self._spin(form, "scale", "Scale", settings.get("scale", 1.0), 0.0001, 1000.0, 4)
            self._combo(form, "up_axis", "Up axis", ["Z", "Y"], settings.get("up_axis", "Z"))
            self._check(form, "quads", "Quads where possible", settings.get("quads", False))
            self._check(form, "ngons", "N-gons where possible", settings.get("ngons", False))
            self._check(form, "mtl", "Write MTL", settings.get("mtl", True))
            self._check(form, "uvs", "Write UVs", settings.get("uvs", True))
        elif fmt == "step":
            self._combo(form, "schema", "Schema", ["AP203", "AP214", "AP242"], settings.get("schema", "AP214"))
            self._check(form, "names", "Write names", settings.get("names", True))
            self._check(form, "colors", "Write colours", settings.get("colors", True))
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        form.addRow(bb)

    def _combo(self, form, key, label, options, value):
        c = QComboBox()
        c.addItems(options)
        c.setCurrentText(str(value))
        form.addRow(label, c)
        self.widgets[key] = c

    def _spin(self, form, key, label, value, lo, hi, decimals):
        s = QDoubleSpinBox()
        s.setRange(lo, hi)
        s.setDecimals(decimals)
        s.setValue(value)
        form.addRow(label, s)
        self.widgets[key] = s

    def _check(self, form, key, label, value):
        c = QCheckBox(label)
        c.setChecked(bool(value))
        form.addRow("", c)
        self.widgets[key] = c

    def values(self) -> dict:
        out = {}
        for k, w in self.widgets.items():
            if isinstance(w, QComboBox):
                v = w.currentText()
                out[k] = (v == "binary") if k == "binary" else v
            elif isinstance(w, QDoubleSpinBox):
                out[k] = w.value()
            elif isinstance(w, QCheckBox):
                out[k] = w.isChecked()
        self.settings.update(out)
        return out


class FastenerDialog(QDialog):
    def __init__(self, parent=None, last: Optional[dict] = None):
        super().__init__(parent)
        self.setWindowTitle("Fastener hole")
        form = QFormLayout(self)
        self.size = QComboBox()
        self.size.addItems(["M2", "M2.5", "M3", "M4", "M5", "M6", "M8"])
        self.kind = QComboBox()
        self.kind.addItems(["clearance", "tap", "counterbore", "countersink", "insert"])
        self.extra = QDoubleSpinBox()
        self.extra.setRange(0.0, 1.0)
        self.extra.setDecimals(2)
        self.extra.setSingleStep(0.05)
        self.depth = QDoubleSpinBox()
        self.depth.setRange(0.0, 500.0)
        self.depth.setSpecialValueText("through")
        if last:
            self.size.setCurrentText(last.get("size", "M3"))
            self.kind.setCurrentText(last.get("kind", "clearance"))
            self.extra.setValue(last.get("extra", 0.0))
            self.depth.setValue(last.get("depth", 0.0))
        form.addRow("Size", self.size)
        form.addRow("Kind", self.kind)
        form.addRow("Extra clearance (mm)", self.extra)
        form.addRow("Depth (mm)", self.depth)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        form.addRow(bb)

    def spec(self):
        from ..printing import FastenerSpec

        return FastenerSpec(self.size.currentText(), self.kind.currentText(), self.extra.value(), self.depth.value() or None)


class ArrayDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Array")
        form = QFormLayout(self)
        self.kind = QComboBox()
        self.kind.addItems(["rectangular", "radial"])
        self.cx, self.cy, self.cz = (QSpinBox() for _ in range(3))
        for s, v in ((self.cx, 3), (self.cy, 1), (self.cz, 1)):
            s.setRange(1, 500)
            s.setValue(v)
        self.mode = QComboBox()
        self.mode.addItems(["count + spacing", "count + total extent"])
        self.sx, self.sy, self.sz = (QLineEdit(v) for v in ("10", "10", "10"))
        self.count = QSpinBox()
        self.count.setRange(2, 360)
        self.count.setValue(6)
        self.angle = QLineEdit("360")
        self.instances = QCheckBox("As live instances")
        self.merge = QCheckBox("Merge into one body")
        form.addRow("Kind", self.kind)
        form.addRow("Count X / Y / Z", self._row(self.cx, self.cy, self.cz))
        form.addRow("Mode", self.mode)
        form.addRow("Spacing or extent X / Y / Z", self._row(self.sx, self.sy, self.sz))
        form.addRow("Radial count", self.count)
        form.addRow("Radial total angle", self.angle)
        form.addRow("", self.instances)
        form.addRow("", self.merge)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        form.addRow(bb)

    def _row(self, *ws):
        w = QWidget()
        h = QHBoxLayout(w)
        h.setContentsMargins(0, 0, 0, 0)
        for x in ws:
            h.addWidget(x)
        return w


# ---- robotics -------------------------------------------------------------
JOINT_TYPE_HINTS = {
    "revolute": "hinge with angle limits (servo, geared motor)",
    "continuous": "hinge without limits (wheel, stepper)",
    "prismatic": "slider along the axis (linear actuator)",
    "fixed": "rigid attachment: the child moves with the parent",
    "ball": "3-DoF spherical joint (no motor)",
}


class MotorDialog(QDialog):
    """Pick a motor from the library and how to place it. `bodies` lists
    `(id, name)` candidates for the mounting body."""

    def __init__(self, parent=None, bodies: Optional[list] = None, last: Optional[dict] = None):
        super().__init__(parent)
        from ..robotics import MOTOR_LIBRARY

        self.setWindowTitle("Add motor")
        form = QFormLayout(self)
        self.spec = QComboBox()
        for sid, m in MOTOR_LIBRARY.items():
            self.spec.addItem(f"{m.name}   {m.kind}  {m.stall_torque:g} N·m  {m.mass_g:g} g", sid)
        self.rotation = QDoubleSpinBox()
        self.rotation.setRange(-360.0, 360.0)
        self.rotation.setSuffix("°")
        self.cut = QCheckBox("Cut mounting holes and pilot into the mounted body")
        self.cut.setChecked(True)
        self.mount = QComboBox()
        self.mount.addItem("(pick by clicking a face)", None)
        for bid, name in bodies or []:
            self.mount.addItem(name, bid)
        self.name = QLineEdit()
        self.name.setPlaceholderText("motor name (optional)")
        self.notes = QLabel("")
        self.notes.setWordWrap(True)
        self.spec.currentIndexChanged.connect(self._update_notes)
        if last:
            i = self.spec.findData(last.get("spec"))
            if i >= 0:
                self.spec.setCurrentIndex(i)
            self.rotation.setValue(last.get("rotation", 0.0))
            self.cut.setChecked(last.get("cut", True))
        self._update_notes()
        form.addRow("Motor", self.spec)
        form.addRow("Rotation about shaft", self.rotation)
        form.addRow("Mount on", self.mount)
        form.addRow("", self.cut)
        form.addRow("Name", self.name)
        form.addRow(self.notes)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        form.addRow(bb)

    def _update_notes(self, *_):
        from ..robotics import MOTOR_LIBRARY

        m = MOTOR_LIBRARY[self.spec.currentData()]
        holes = f"{len(m.mount_holes)} mount holes" if m.mount_holes else "no mount holes"
        self.notes.setText(f"{m.shape}, {m.size[0]:g}×{m.size[1]:g}×{m.size[2]:g} mm, shaft Ø{m.shaft_diameter:g}×{m.shaft_length:g} mm, {holes}, {m.no_load_speed:g} rad/s no-load, {m.voltage:g} V. {m.notes}")

    def values(self) -> dict:
        return {"spec": self.spec.currentData(), "rotation": self.rotation.value(), "cut": self.cut.isChecked(), "mount_on": self.mount.currentData(), "name": self.name.text().strip() or None}


class JointDialog(QDialog):
    """Joint type, the two bodies, pivot/axis, limits and the driving motor.
    `bodies` and `motors` are `(id, name)` lists; `preset` prefills fields."""

    def __init__(self, parent=None, bodies: Optional[list] = None, motors: Optional[list] = None, preset: Optional[dict] = None, title: str = "Add joint"):
        super().__init__(parent)
        from ..robotics import JOINT_TYPES

        preset = preset or {}
        self.setWindowTitle(title)
        form = QFormLayout(self)
        self.type = QComboBox()
        for t in JOINT_TYPES:
            self.type.addItem(f"{t}: {JOINT_TYPE_HINTS.get(t, '')}", t)
        self.parent_box = QComboBox()
        self.parent_box.addItem("(world)", None)
        self.child_box = QComboBox()
        for bid, name in bodies or []:
            self.parent_box.addItem(name, bid)
            self.child_box.addItem(name, bid)
        self.pivot = [QLineEdit() for _ in range(3)]
        self.axis = [QLineEdit() for _ in range(3)]
        for i, (pv, ax) in enumerate(zip(self.pivot, self.axis)):
            pv.setText(f"{preset.get('pivot', (0.0, 0.0, 0.0))[i]:g}")
            ax.setText(f"{preset.get('axis', (0.0, 0.0, 1.0))[i]:g}")
        self.lower = QLineEdit()
        self.upper = QLineEdit()
        self.lower.setPlaceholderText("none")
        self.upper.setPlaceholderText("none")
        self.motor = QComboBox()
        self.motor.addItem("(none)", None)
        for mid, name in motors or []:
            self.motor.addItem(name, mid)
        self.gear = QDoubleSpinBox()
        self.gear.setRange(0.01, 10000.0)
        self.gear.setDecimals(2)
        self.gear.setValue(1.0)
        self.damping = QDoubleSpinBox()
        self.damping.setRange(0.0, 1000.0)
        self.damping.setDecimals(4)
        self.name = QLineEdit()
        self.name.setPlaceholderText("joint name (optional)")
        i = self.type.findData(preset.get("type", "revolute"))
        self.type.setCurrentIndex(max(0, i))
        for box, key in ((self.parent_box, "parent"), (self.child_box, "child"), (self.motor, "motor")):
            j = box.findData(preset.get(key))
            if j >= 0:
                box.setCurrentIndex(j)
        for w, key in ((self.lower, "lower"), (self.upper, "upper")):
            if preset.get(key) is not None:
                w.setText(f"{math.degrees(preset[key]):g}" if preset.get("type", "revolute") != "prismatic" else f"{preset[key]:g}")
        self.gear.setValue(preset.get("gear_ratio", 1.0))
        self.damping.setValue(preset.get("damping", 0.0))
        self.name.setText(preset.get("name", "") or "")

        def row3(ws):
            h = QHBoxLayout()
            for w in ws:
                h.addWidget(w)
            return h

        form.addRow("Type", self.type)
        form.addRow("Parent", self.parent_box)
        form.addRow("Child", self.child_box)
        form.addRow("Pivot (mm)", row3(self.pivot))
        form.addRow("Axis", row3(self.axis))
        form.addRow("Lower limit (° or mm)", self.lower)
        form.addRow("Upper limit (° or mm)", self.upper)
        form.addRow("Motor", self.motor)
        form.addRow("Extra gear ratio", self.gear)
        form.addRow("Damping (N·m·s/rad)", self.damping)
        form.addRow("Name", self.name)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        form.addRow(bb)

    def values(self) -> dict:
        t = self.type.currentData()

        def limit(w):
            txt = w.text().strip()
            if not txt:
                return None
            v = evaluate(txt)
            return v if t == "prismatic" else math.radians(v)

        return {
            "type": t,
            "parent": self.parent_box.currentData(),
            "child": self.child_box.currentData(),
            "pivot": tuple(evaluate(w.text() or "0") for w in self.pivot),
            "axis": tuple(evaluate(w.text() or "0") for w in self.axis),
            "lower": limit(self.lower),
            "upper": limit(self.upper),
            "motor": self.motor.currentData(),
            "gear_ratio": self.gear.value(),
            "damping": self.damping.value(),
            "name": self.name.text().strip() or None,
        }


class RobotPanel(QWidget):
    """Joints, motors, degrees of freedom and validation issues, with the
    robotics commands as buttons. Double-click a joint to edit it."""

    def __init__(self, app, parent=None):
        super().__init__(parent)
        self.app = app
        lay = QVBoxLayout(self)
        lay.setContentsMargins(4, 4, 4, 4)
        self.summary = QLabel("")
        self.summary.setWordWrap(True)
        lay.addWidget(self.summary)
        self.tree = QTreeWidget()
        self.tree.setHeaderLabels(["Item", "Detail", "Margin"])
        self.tree.setColumnWidth(0, 150)
        self.tree.setColumnWidth(1, 260)
        self.tree.itemSelectionChanged.connect(self._select)
        self.tree.itemDoubleClicked.connect(self._edit)
        lay.addWidget(self.tree, 3)
        self.issues = QListWidget()
        lay.addWidget(self.issues, 1)
        grid = QGridLayout()
        buttons = [
            ("Add joint…", "robot.add_joint"), ("Add motor…", "robot.add_motor"),
            ("Joint from selection…", "robot.joint_dialog"), ("Infer joints", "robot.infer"),
            ("Assign motor…", "robot.assign_motor"), ("Fix together", "robot.fixed"),
            ("Toggle ground", "robot.ground"), ("Add sensor…", "robot.add_sensor"),
            ("Add cable…", "robot.add_cable"), ("Battery / control…", "robot.power"),
            ("Export sim…", "sim.export"), ("Stress overlay", "view.stress"),
            ("Load results…", "robot.load_results"), ("Apply identification…", "robot.apply_identification"),
        ]
        for i, (label, cid) in enumerate(buttons):
            b = QPushButton(label)
            b.clicked.connect(lambda *_, c=cid: self.app.commands[c]["run"]())
            grid.addWidget(b, i // 2, i % 2)
        lay.addLayout(grid)

    def refresh(self, *_):
        self.tree.blockSignals(True)
        self.tree.clear()
        try:
            info = self.app.ops.robot()
        except Exception as e:  # never let a panel break the app
            self.summary.setText(str(e))
            self.tree.blockSignals(False)
            return
        from ..physical import results_margins

        margins = results_margins(self.app.doc)
        doc = self.app.doc
        ground = ", ".join(doc.nodes[g].name for g in info["ground"] if g in doc.nodes) or "none (heaviest root body is used)"
        sensors = [n for n in doc.walk() if n.kind == "sensor"]
        cables = [n for n in doc.walk() if n.kind == "cable"]
        st = doc.robot_settings or {}
        power = f"{st['battery']['nominal_voltage']:g} V {st['battery']['chemistry']}" if st.get("battery") else "no battery (motor supply voltage)"
        res = f"  Results: {os.path.basename(doc.results.get('path', ''))} ({doc.results.get('duration_s', '?')} s run)" if doc.results else ""
        mobility = f"{info['dof']} DoF" if info['dof'] is not None else 'closed-loop mobility requires constraint analysis'
        self.summary.setText(f"{info['links']} bodies, {len(info['joints'])} joints, {mobility}, {len(info['motors'])} motors, {len(sensors)} sensors, {len(cables)} cables. Ground: {ground}. Power: {power}.{res}")

        def margin_text(nid):
            m = margins.get(nid)
            if not m:
                return ""
            parts = []
            for key, label in (("yield_margin", "yield"), ("bearing_margin", "bearing"), ("screw_shear_margin", "screw"), ("stall_margin", "stall"), ("tg_margin_c", "Tg"), ("mount_tg_margin_c", "mount Tg")):
                v = m.get(key)
                if v is not None:
                    parts.append(f"{label} {v:+.2f}" if "tg" not in key else f"{label} {v:+.0f}°C")
            return "  ".join(parts)

        lt = QTreeWidgetItem(["Links", "", ""])
        for n in doc.bodies():
            if (n.robot or {}).get("kind") == "motor":
                continue
            r = n.results if n.results and n.results.get("section") == "links" else None
            detail = f"{doc.materials[n.material].name if n.material in doc.materials else '—'}" + (f", peak {r['peak_stress_pa'] / 1e6:.1f} MPa" if r and r.get("peak_stress_pa") is not None else "") + (f", {r['peak_temperature_c']:.0f} °C" if r and r.get("peak_temperature_c") is not None else "")
            it = QTreeWidgetItem([f"▣ {n.name}", detail, margin_text(n.id)])
            it.setData(0, Qt.UserRole, n.id)
            lt.addChild(it)
        self.tree.addTopLevelItem(lt)
        jt = QTreeWidgetItem(["Joints", "", ""])
        for j in info["joints"]:
            lim = ""
            if j.get("lower") is not None or j.get("upper") is not None:
                f = (lambda v: f"{math.degrees(v):.0f}°") if j["type"] != "prismatic" else (lambda v: f"{v:g} mm")
                lim = f"  [{f(j['lower']) if j.get('lower') is not None else '…'}, {f(j['upper']) if j.get('upper') is not None else '…'}]"
            detail = f"{j['type']}: {j.get('parent_name') or 'world'} → {j.get('child_name')}{lim}" + (f"  ⚡{j['motor_name']}" if j.get("motor_name") else "")
            it = QTreeWidgetItem([f"⚙ {j['name']}", detail, margin_text(j["id"])])
            it.setData(0, Qt.UserRole, j["id"])
            jt.addChild(it)
        mt = QTreeWidgetItem(["Motors", "", ""])
        for m in info["motors"]:
            on = self.app.doc.nodes[m["mounted_on"]].name if m.get("mounted_on") in self.app.doc.nodes else "loose"
            drives = self.app.doc.nodes[m["drives"]].name if m.get("drives") in self.app.doc.nodes else "no joint"
            it = QTreeWidgetItem([f"⚡ {m['name']}", f"{m.get('spec_name')}: on {on}, drives {drives}", margin_text(m["id"])])
            it.setData(0, Qt.UserRole, m["id"])
            mt.addChild(it)
        self.tree.addTopLevelItem(jt)
        self.tree.addTopLevelItem(mt)
        if sensors or cables:
            xt = QTreeWidgetItem(["Sensors & cables", "", ""])
            for n in sensors:
                r = n.robot
                it = QTreeWidgetItem([f"◎ {n.name}", f"{r['kind']} on {doc.nodes[r['body']].name if r['body'] in doc.nodes else '?'}" + (f", reads {r['joint_name']}" if r.get("joint_name") else ""), ""])
                it.setData(0, Qt.UserRole, n.id)
                xt.addChild(it)
            for n in cables:
                r = n.robot
                it = QTreeWidgetItem([f"〜 {n.name}", f"{doc.nodes[r['from_body']].name if r['from_body'] in doc.nodes else '?'} → {doc.nodes[r['to_body']].name if r['to_body'] in doc.nodes else '?'}", ""])
                it.setData(0, Qt.UserRole, n.id)
                xt.addChild(it)
            self.tree.addTopLevelItem(xt)
        self.tree.expandAll()
        self.tree.blockSignals(False)
        self.issues.clear()
        for i in info["issues"]:
            it = QListWidgetItem(("⛔ " if i["severity"] == "error" else "⚠ ") + i["message"])
            it.setData(Qt.UserRole, i.get("node"))
            it.setForeground(QColor(255, 110, 100) if i["severity"] == "error" else QColor(255, 200, 90))
            self.issues.addItem(it)
        if not info["issues"] and info["joints"]:
            self.issues.addItem(QListWidgetItem("✓ robot is valid"))

    def _select(self):
        ids = [it.data(0, Qt.UserRole) for it in self.tree.selectedItems() if it.data(0, Qt.UserRole)]
        if ids:
            self.app.viewport.selection.set_nodes(ids)
            self.app.viewport.update()

    def _edit(self, item, _col):
        nid = item.data(0, Qt.UserRole)
        if nid and self.app.doc.nodes[nid].kind == "joint":
            self.app.robot_edit_joint(nid)


class SensorDialog(QDialog):
    def __init__(self, parent=None, bodies=None, joints=None, preset=None):
        super().__init__(parent)
        preset = preset or {}
        self.setWindowTitle("Add sensor")
        form = QFormLayout(self)
        self.kind = QComboBox()
        for k, hint in (("imu", "IMU: accelerometer + gyro with noise, bias and quantisation"), ("encoder", "encoder on a joint"), ("current", "motor current sense"), ("force", "load cell / foot force")):
            self.kind.addItem(f"{k}: {hint}", k)
        self.body = QComboBox()
        for bid, name in bodies or []:
            self.body.addItem(name, bid)
        self.joint = QComboBox()
        self.joint.addItem("(none)", None)
        for jid, name in joints or []:
            self.joint.addItem(name, jid)
        self.point = [QLineEdit(f"{preset.get('point', (0, 0, 0))[i]:g}") for i in range(3)]
        self.rate = QDoubleSpinBox()
        self.rate.setRange(1.0, 100000.0)
        self.rate.setValue(200.0)
        self.name = QLineEdit()
        if preset.get("body"):
            self.body.setCurrentIndex(max(0, self.body.findData(preset["body"])))
        row = QHBoxLayout()
        for w in self.point:
            row.addWidget(w)
        form.addRow("Kind", self.kind)
        form.addRow("On body", self.body)
        form.addRow("Point (mm)", row)
        form.addRow("Reads joint", self.joint)
        form.addRow("Rate (Hz)", self.rate)
        form.addRow("Name", self.name)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        form.addRow(bb)

    def values(self) -> dict:
        return {"kind": self.kind.currentData(), "body": self.body.currentData(), "point": tuple(evaluate(w.text() or "0") for w in self.point), "joint": self.joint.currentData(), "rate_hz": self.rate.value(), "name": self.name.text().strip() or None}


class CableDialog(QDialog):
    def __init__(self, parent=None, bodies=None, preset=None):
        super().__init__(parent)
        preset = preset or {}
        self.setWindowTitle("Add cable")
        form = QFormLayout(self)
        self.a, self.b = QComboBox(), QComboBox()
        for bid, name in bodies or []:
            self.a.addItem(name, bid)
            self.b.addItem(name, bid)
        self.pa = [QLineEdit(f"{preset.get('from_point', (0, 0, 0))[i]:g}") for i in range(3)]
        self.pb = [QLineEdit(f"{preset.get('to_point', (0, 0, 0))[i]:g}") for i in range(3)]
        self.length = QLineEdit("")
        self.length.setPlaceholderText("auto: 10 % slack")
        self.mass = QLineEdit("")
        self.mass.setPlaceholderText("auto: 4 g per 100 mm")
        self.name = QLineEdit()
        if preset.get("from_body"):
            self.a.setCurrentIndex(max(0, self.a.findData(preset["from_body"])))
        if preset.get("to_body"):
            self.b.setCurrentIndex(max(0, self.b.findData(preset["to_body"])))

        def row(ws):
            h = QHBoxLayout()
            for w in ws:
                h.addWidget(w)
            return h

        form.addRow("From body", self.a)
        form.addRow("From point (mm)", row(self.pa))
        form.addRow("To body", self.b)
        form.addRow("To point (mm)", row(self.pb))
        form.addRow("Length (mm)", self.length)
        form.addRow("Mass (g)", self.mass)
        form.addRow("Name", self.name)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        form.addRow(bb)

    def values(self) -> dict:
        return {
            "from_body": self.a.currentData(), "from_point": tuple(evaluate(w.text() or "0") for w in self.pa),
            "to_body": self.b.currentData(), "to_point": tuple(evaluate(w.text() or "0") for w in self.pb),
            "length": float(evaluate(self.length.text())) if self.length.text().strip() else None,
            "mass": float(evaluate(self.mass.text())) * 1e-3 if self.mass.text().strip() else None,
            "name": self.name.text().strip() or None,
        }


class PowerDialog(QDialog):
    """Battery, control loop and Monte Carlo uncertainty."""

    def __init__(self, parent=None, settings=None, joints=None):
        super().__init__(parent)
        from ..physical import default_settings

        st = {**default_settings(), **(settings or {})}
        self.setWindowTitle("Battery, control and uncertainty")
        form = QFormLayout(self)
        bat = st.get("battery") or {}
        self.cells = QSpinBox()
        self.cells.setRange(0, 24)
        self.cells.setSpecialValueText("none (motor supply voltage)")
        self.cells.setValue(int(bat.get("cells", 0)))
        self.chem = QComboBox()
        self.chem.addItems(["lipo", "liion", "lifepo4", "nimh", "alkaline"])
        self.chem.setCurrentText(bat.get("chemistry", "lipo"))
        self.capacity = QDoubleSpinBox()
        self.capacity.setRange(0.01, 100.0)
        self.capacity.setValue(float(bat.get("capacity_ah", 1.0)))
        ctl = st.get("control") or {}
        self.period = QDoubleSpinBox()
        self.period.setDecimals(4)
        self.period.setRange(0.0001, 1.0)
        self.period.setValue(float(ctl.get("period_s", 0.02)))
        self.latency = QDoubleSpinBox()
        self.latency.setDecimals(4)
        self.latency.setRange(0.0, 1.0)
        self.latency.setValue(float(ctl.get("latency_s", 0.004)))
        self.targets = {}
        for jid, name in joints or []:
            e = QLineEdit(f"{math.degrees(float((ctl.get('targets') or {}).get(name, 0.0))):g}")
            self.targets[name] = e
        unc = st.get("uncertainty") or {}
        self.dim = QDoubleSpinBox()
        self.dim.setDecimals(3)
        self.dim.setRange(0.0, 2.0)
        self.dim.setValue(float((unc.get("dimension_m") or {}).get("sigma", 0.15e-3)) * 1e3)
        self.fric = QDoubleSpinBox()
        self.fric.setRange(0.0, 1.0)
        self.fric.setValue(float((unc.get("friction") or {}).get("sigma_fraction", 0.2)))
        form.addRow("Battery cells (series)", self.cells)
        form.addRow("Chemistry", self.chem)
        form.addRow("Capacity (Ah)", self.capacity)
        form.addRow("Control period (s)", self.period)
        form.addRow("Control latency (s)", self.latency)
        for name, e in self.targets.items():
            form.addRow(f"Target {name} (°)", e)
        form.addRow("Dimension σ (mm)", self.dim)
        form.addRow("Friction σ (fraction)", self.fric)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(self.accept)
        bb.rejected.connect(self.reject)
        form.addRow(bb)

    def apply(self, ops):
        if self.cells.value() > 0:
            ops.set_battery(cells=self.cells.value(), chemistry=self.chem.currentText(), capacity_ah=self.capacity.value())
        else:
            ops.set_robot_setting("battery", None)
        targets = {name: math.radians(float(evaluate(e.text() or "0"))) for name, e in self.targets.items()}
        ops.set_control(period_s=self.period.value(), latency_s=self.latency.value(), targets=targets)
        ops.set_uncertainty(dimension_m=self.dim.value() * 1e-3, friction=self.fric.value())
