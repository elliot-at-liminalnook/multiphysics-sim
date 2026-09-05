"""In-editor annotation placement, threaded discussion, and viewport pins."""
import numpy as np
import html
from copy import deepcopy
from PySide6.QtCore import QPointF, QRectF, Qt, QSize
from PySide6.QtGui import QColor, QFont, QPen
from PySide6.QtWidgets import (QCheckBox, QComboBox, QHBoxLayout, QLabel, QLineEdit,
    QListWidget, QListWidgetItem, QPlainTextEdit, QPushButton, QVBoxLayout, QWidget, QInputDialog)

from .tools import Tool, SelectTool
from ..annotations import thread_parts, PART_LINK
from ..saved_views import capture_view, restore_view


def comment_html(body, doc):
    out, start = [], 0
    for match in PART_LINK.finditer(body):
        out.append(html.escape(body[start:match.start()]))
        label, nid = match.groups()
        if nid in doc.nodes:
            out.append(f'<a style="color:#74c8ef" href="part:{nid}" title="View this part">{html.escape(label)}</a>')
        else:
            out.append(html.escape(label) + ' (part deleted)')
        start = match.end()
    out.append(html.escape(body[start:]))
    return ''.join(out).replace('\n', '<br>')


class MessageLabel(QLabel):
    def mousePressEvent(self, event):
        self.message_list.setCurrentItem(self.message_item)
        super().mousePressEvent(event)


class MessageList(QListWidget):
    def size_messages(self):
        width = max(120, self.viewport().width() - 20)
        for i in range(self.count()):
            item = self.item(i); widget = self.itemWidget(item)
            if widget:
                widget.setFixedWidth(width)
                item.setSizeHint(QSize(width, widget.heightForWidth(width) + 14))

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self.size_messages()


def saved_view(vp):
    c = vp.camera
    return {"target": list(c.target), "distance": c.distance, "yaw": c.yaw,
            "pitch": c.pitch, "fov": c.fov, "orthographic": c.orthographic,
            "mode": c.mode, "rot": c.rot.tolist()}


class AnnotateTool(Tool):
    name = "annotate"
    hint = "Click a surface to place a comment • click a pin to read it • Esc cancels"

    def __init__(self, ctx, thread_id=None):
        super().__init__(ctx)
        self.thread_id = thread_id

    def activate(self):
        self.previous_mode = self.ctx.vp.selection_mode
        self.ctx.vp.selection_mode = "face"
        super().activate()

    def deactivate(self):
        self.ctx.vp.selection_mode = self.previous_mode
        super().deactivate()

    def release(self, pos, mods):
        def done(result):
            if self.ctx.app.tool is not self:
                return
            hit, point = result.get("hit"), result.get("world")
            if not hit or point is None:
                self.ctx.error("Click a visible surface to place the annotation")
                return
            kind, nid, index = hit
            self.ctx.app.comments.begin(nid, point, index if kind == "face" else None, self.thread_id)
        self.ctx.vp.request_pick(pos.x(), pos.y(), done)


class CommentsPanel(QWidget):
    def __init__(self, app):
        super().__init__(app)
        self.app = app
        self.pending = None
        self.editing = None
        self.pin_hits = []
        self.cached = []
        self.draft_actions = []
        self._inspection_context = None
        layout = QVBoxLayout(self)
        layout.setContentsMargins(12, 12, 12, 12)
        top = QHBoxLayout()
        self.add = QPushButton("＋ Annotate model")
        self.add.clicked.connect(lambda: app.set_tool(AnnotateTool(app.ctx)))
        self.add.setToolTip("Click a face in the viewport, then write a comment (N)")
        top.addWidget(self.add)
        self.filter = QComboBox()
        self.filter.addItems(["Open", "All", "Resolved"])
        self.filter.currentIndexChanged.connect(self.refresh)
        top.addWidget(self.filter)
        layout.addLayout(top)
        self.selected_only = QCheckBox("Selected parts only")
        self.selected_only.toggled.connect(self.refresh)
        layout.addWidget(self.selected_only)
        self.threads = QListWidget()
        self.threads.setWordWrap(True)
        self.threads.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.threads.setMinimumHeight(110)
        self.threads.currentItemChanged.connect(self.show_thread)
        self.threads.itemDoubleClicked.connect(lambda *_: self.focus_thread())
        layout.addWidget(self.threads)
        self.location = QLabel("Select Annotate model, then click a surface.")
        self.location.setWordWrap(True)
        layout.addWidget(self.location)
        nav = QHBoxLayout()
        for title, callback in [("Show on model", self.focus_thread), ("Fit in view", self.fit_thread), ("Reattach…", self.reattach), ("Resolve", self.resolve)]:
            b = QPushButton(title)
            b.clicked.connect(lambda checked=False, cb=callback: app._safe(cb))
            nav.addWidget(b)
            self.draft_actions.append(b)
            if title == "Fit in view":
                self.fit_button = b
                b.setToolTip("Center and zoom to the annotated part, keeping the current viewing angle")
            if title == "Resolve": self.resolve_button = b
        layout.addLayout(nav)
        layout.addWidget(QLabel('Parts in this discussion · double-click to view a part'))
        self.parts = QListWidget()
        self.parts.setWordWrap(True)
        self.parts.setMaximumHeight(135)
        self.parts.itemClicked.connect(lambda item: app._safe(lambda: self.highlight_parts([item.data(Qt.UserRole)])))
        self.parts.itemDoubleClicked.connect(lambda item: app._safe(lambda: self.view_parts([item.data(Qt.UserRole)])))
        layout.addWidget(self.parts)
        for entries in [ [('Link selected parts', self.link_selection), ('Rename part label…', self.label_part)],
                         [('Show only linked parts', self.view_parts), ('Return to assembly', self.end_inspection)] ]:
            row = QHBoxLayout()
            for title, callback in entries:
                button = QPushButton(title)
                button.clicked.connect(lambda checked=False, cb=callback: app._safe(cb))
                row.addWidget(button)
                if title == 'Return to assembly': self.return_button = button
                else: self.draft_actions.append(button)
            layout.addLayout(row)
        self.return_button.setEnabled(False)
        self.messages = MessageList()
        self.messages.setWordWrap(True)
        self.messages.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.messages.setMinimumHeight(130)
        layout.addWidget(self.messages)
        self.author = QLineEdit("You")
        self.author.setPlaceholderText("Your display name")
        self.author.setAccessibleName("Comment author")
        layout.addWidget(self.author)
        self.insert_link = QPushButton('Insert part link from selection')
        self.insert_link.setToolTip('Select parts in the outliner or viewport, then insert clickable names into your comment')
        self.insert_link.clicked.connect(lambda: app._safe(self.insert_part_link))
        layout.addWidget(self.insert_link)
        self.editor = QPlainTextEdit()
        self.editor.setPlaceholderText("Write a reply…")
        self.editor.setMaximumHeight(110)
        self.editor.setAccessibleName("Annotation message")
        self.editor.textChanged.connect(self.update_send)
        layout.addWidget(self.editor)
        row = QHBoxLayout()
        self.send = QPushButton("Reply")
        self.send.setObjectName("primaryAction")
        self.send.clicked.connect(lambda: app._safe(self.submit))
        self.cancel = QPushButton("Cancel")
        self.cancel.clicked.connect(self.cancel_draft)
        row.addWidget(self.send)
        row.addWidget(self.cancel)
        layout.addLayout(row)
        row = QHBoxLayout()
        for title, callback in [("Edit message", self.edit_message), ("Delete message", self.delete_message), ("Delete thread", self.delete_thread)]:
            b = QPushButton(title)
            b.setToolTip(title + " • Undo is available")
            b.clicked.connect(lambda checked=False, cb=callback: app._safe(cb))
            row.addWidget(b)
            self.draft_actions.append(b)
        layout.addLayout(row)
        self.update_send()
        app.viewport.overlays.append(self.draw_pins)

    def current_id(self):
        item = self.threads.currentItem()
        return item.data(Qt.UserRole) if item else None

    def update_send(self):
        self.send.setEnabled(bool(self.editor.toPlainText().strip()) and bool(self.pending or self.current_id()))
        drafting = bool(self.editor.toPlainText() or self.pending or self.editing)
        for widget in [self.threads, self.filter, self.selected_only, self.add, *self.draft_actions]:
            widget.setEnabled(not drafting)
        self.fit_button.setEnabled(not drafting and bool(self._fit_nodes()))

    def _fit_nodes(self):
        t = self.app.doc.annotations.get(self.current_id())
        if not t:
            return []
        ids = [r['node_id'] for r in thread_parts(t)]
        return [i for i in ids if i in self.app.doc.nodes]

    def refresh(self, *_):
        tid = self.current_id()
        self.cached = self.app.ops.threads()
        self.threads.blockSignals(True)
        self.threads.clear()
        selected = set(self.app.viewport.selection.nodes())
        status = {"Open": "open", "Resolved": "resolved"}.get(self.filter.currentText())
        for number, t in enumerate(self.cached, 1):
            if status and t["status"] != status: continue
            if self.selected_only.isChecked() and t["anchor"]["node_id"] not in selected and not selected.intersection(r['node_id'] for r in thread_parts(t)): continue
            prefix = "✓" if t["status"] == "resolved" else str(number)
            preview = PART_LINK.sub(lambda match: match.group(1), t['comments'][0]['body'])
            item = QListWidgetItem(f"{prefix} · {t['node_name']}\n{preview[:90]}")
            item.setData(Qt.UserRole, t["id"])
            self.threads.addItem(item)
            if t["id"] == tid: self.threads.setCurrentItem(item)
        self.threads.blockSignals(False)
        if not self.pending and not self.editing:
            self.show_thread()
        self.app.viewport.update()

    def show_thread(self, *_):
        selected_message = self.messages.currentItem().data(Qt.UserRole) if self.messages.currentItem() else None
        self.messages.clear()
        selected_part = self.parts.currentItem().data(Qt.UserRole) if self.parts.currentItem() else None
        self.parts.clear()
        tid = self.current_id()
        t = next((t for t in self.cached if t["id"] == tid), None)
        if t:
            for ref in thread_parts(t):
                node = self.app.doc.nodes.get(ref['node_id'])
                label = ref.get('label') or (node.name if node else 'Deleted part')
                description = ref.get('description') or (node.name if node and node.name != label else '')
                item = QListWidgetItem(label + ('\n' + description if description else '') + (' · deleted' if node is None else ''))
                item.setData(Qt.UserRole, ref['node_id'])
                item.setToolTip((node.name + '\n' if node else '') + 'Click to highlight · double-click to view alone')
                if node is None: item.setFlags(item.flags() & ~Qt.ItemIsEnabled)
                self.parts.addItem(item)
                if ref['node_id'] == selected_part: self.parts.setCurrentItem(item)
            state = {"evidence": "Captured experiment", "attached": "Attached to surface", "missing": "Part deleted — reattach this annotation", "needs_review": "Geometry changed — check and reattach this pin"}[t["anchor_status"]]
            self.location.setText(f"{t['node_name']} · {state}")
            if t.get('evidence'):
                ev = t['evidence']
                self.location.setText(self.location.text() + f"\nRun {ev['run_id'][:8]} · {ev.get('signal', '')} · {ev.get('time_range', [])} s")
            self.resolve_button.setText("Reopen" if t["status"] == "resolved" else "Resolve")
            for c in t["comments"]:
                item = QListWidgetItem(f"{c['author']} · {c['created_at'][:16].replace('T', ' ')}\n{c['body']}")
                item.setData(Qt.UserRole, c["id"])
                self.messages.addItem(item)
                if c['id'] == selected_message: self.messages.setCurrentItem(item)
                label = MessageLabel()
                label.message_list, label.message_item = self.messages, item
                label.setWordWrap(True)
                label.setTextFormat(Qt.RichText)
                label.setOpenExternalLinks(False)
                label.setTextInteractionFlags(Qt.TextBrowserInteraction)
                label.setText('<b>' + html.escape(c['author']) + '</b> · ' + html.escape(c['created_at'][:16].replace('T', ' ')) + '<br>' + comment_html(c['body'], self.app.doc))
                label.linkActivated.connect(lambda link: self.app._safe(lambda: self.open_part_link(link)))
                self.messages.setItemWidget(item, label)
            self.messages.size_messages()
        elif not self.pending:
            self.location.setText("Click Annotate model to start a discussion on a surface.")
        self.update_send()
        self.app.viewport.update()

    def select(self, tid):
        if (self.pending or self.editor.toPlainText() or self.editing) and tid != self.current_id():
            self.app.status("Post or cancel your current draft before opening another thread")
            return
        self.app.comments_dock.show()
        self.app.comments_dock.raise_()
        self.filter.setCurrentText("All")
        self.selected_only.setChecked(False)
        self.refresh()
        for k in range(self.threads.count()):
            item = self.threads.item(k)
            if item.data(Qt.UserRole) == tid:
                self.threads.setCurrentItem(item)
                break

    def begin(self, nid, point, face=None, thread_id=None):
        view = saved_view(self.app.viewport)
        self.app.set_tool(SelectTool(self.app.ctx))
        if self.pending or self.editor.toPlainText() or self.editing:
            self.app.status("Post or cancel your current draft before placing another pin")
            self.editor.setFocus()
            return
        self.app.comments_dock.show()
        self.app.comments_dock.raise_()
        if thread_id:
            self.app.ops.update_thread(thread_id, node_id=nid, point=list(point), face=face, view=view)
            self.select(thread_id)
            return
        self.pending = {"node_id": nid, "point": list(point), "face": face, "view": view}
        self.editing = None
        self.location.setText(f"New annotation on {self.app.doc.nodes[nid].name}")
        self.editor.setPlaceholderText("What would you like to discuss about this surface?")
        self.send.setText("Post annotation")
        self.update_send()
        self.editor.setFocus()
        self.app.status("Pin placed • write your annotation, then Post annotation")

    def submit(self):
        body, author = self.editor.toPlainText(), self.author.text()
        if self.pending:
            tid = self.app.ops.create_thread(body=body, author=author, **self.pending)
        elif self.editing:
            tid = self.app.ops.update_comment(self.editing, body)
        else:
            tid = self.current_id()
            if not tid: return
            self.app.ops.add_comment(tid, body, author)
        self.cancel_draft()
        self.select(tid)
        self.app.status("Annotation saved in document • Ctrl+S writes the file • Ctrl+Z undoes")

    def cancel_draft(self):
        self.pending = self.editing = None
        self.editor.clear()
        self.editor.setPlaceholderText("Write a reply…")
        self.send.setText("Reply")
        self.show_thread()
        self.app.viewport.update()

    def focus_thread(self):
        tid = self.current_id()
        if not tid: return
        t = self.app.doc.annotations[tid]
        if t.get('evidence') and hasattr(self.app, 'experiments_panel'):
            self.app.experiments_panel.open_evidence(t['evidence'])
            return
        vp = self.app.viewport
        self.end_inspection()
        nid = t["anchor"]["node_id"]
        if nid in self.app.doc.nodes:
            vp.selection.set_nodes([nid])
            self.app.selection_changed(None)
        for key in ("target", "distance", "yaw", "pitch", "fov", "orthographic"):
            value = t["view"].get(key)
            if value is not None: setattr(vp.camera, key, tuple(value) if key == "target" else value)
        vp.camera.mode = t["view"].get("mode", "turntable")
        if "rot" in t["view"]:
            vp.camera.rot = np.array(t["view"]["rot"], dtype=float)
        if t.get('inspection_view'): restore_view(self.app, t['inspection_view'])
        vp.update()

    def _expand_parts(self, ids):
        return list(dict.fromkeys(n.id for nid in ids if nid in self.app.doc.nodes
                                 for n in [self.app.doc.nodes[nid], *self.app.doc.walk(nid)]))

    def highlight_parts(self, ids=None):
        ids = self._expand_parts(self._fit_nodes() if ids is None else ids)
        self.app.viewport.selection.set_nodes(ids)
        self.app.selection_changed(None)

    def view_parts(self, ids=None):
        ids = self._fit_nodes() if ids is None else [i for i in ids if i in self.app.doc.nodes]
        if not ids:
            self.app.status('No available linked parts to show')
            return
        vp = self.app.viewport
        if self._inspection_context is None:
            self._inspection_context = (capture_view(vp), deepcopy(vp.selection.items), vp.inspection_ids)
        t = self.app.doc.annotations.get(self.current_id(), {})
        ref = next((r for r in thread_parts(t) if r['node_id'] == ids[0]), {}) if len(ids) == 1 and t else {}
        if ref.get('view'): restore_view(self.app, ref['view'])
        else:
            vp.section_enabled = False
            vp.camera.orthographic = True
        vp.inspection_ids = set(self._expand_parts(ids))
        if not ref.get('view'): vp.focus_nodes(ids)
        self.highlight_parts(ids)
        vp.tool_name = 'Part inspection'
        vp.tool_hint = 'Showing linked parts only · Esc or Return to assembly restores your view'
        self.return_button.setEnabled(True)
        self.app.status('Part view: ' + ', '.join(self.app.doc.nodes[i].name for i in ids))

    def end_inspection(self):
        if self._inspection_context is None: return
        state, selection, isolated = self._inspection_context
        self._inspection_context = None
        restore_view(self.app, state)
        vp = self.app.viewport
        vp.inspection_ids = isolated
        vp.selection.items = [item for item in selection if item[0] in self.app.doc.nodes]
        self.return_button.setEnabled(False)
        self.app.selection_changed(None)
        self.app._tool_feedback()
        vp.update()

    def open_part_link(self, link):
        if link.startswith('part:'):
            self.view_parts([link[5:]])

    def link_selection(self):
        tid = self.current_id()
        if not tid: return
        refs = thread_parts(self.app.doc.annotations[tid])
        known = {r['node_id'] for r in refs}
        refs.extend({'node_id': nid} for nid in self.app.viewport.selection.nodes() if nid not in known)
        self.app.ops.update_thread(tid, part_refs=refs)

    def label_part(self):
        item, tid = self.parts.currentItem(), self.current_id()
        if not item or not tid: return
        refs = thread_parts(self.app.doc.annotations[tid])
        ref = next(r for r in refs if r['node_id'] == item.data(Qt.UserRole))
        label, ok = QInputDialog.getText(self, 'Part label for this discussion', 'Plain-language label:', text=ref.get('label') or self.app.doc.nodes[ref['node_id']].name)
        if ok:
            ref['label'] = label
            self.app.ops.update_thread(tid, part_refs=refs)

    def insert_part_link(self):
        ids = self.app.viewport.selection.nodes()
        if not ids:
            self.app.status('Select a part in the outliner or viewport first')
            return
        refs = thread_parts(self.app.doc.annotations[self.current_id()]) if self.current_id() else []
        labels = {r['node_id']: r.get('label') for r in refs}
        links = []
        for nid in ids:
            if nid not in self.app.doc.nodes: continue
            label = (labels.get(nid) or self.app.doc.nodes[nid].name).replace('[', '(').replace(']', ')').replace('\n', ' ')
            links.append(f'[{label}](part:{nid})')
        self.editor.insertPlainText(', '.join(links))
        self.editor.setFocus()

    def fit_thread(self):
        ids = self._fit_nodes()
        if not ids:
            return
        vp = self.app.viewport
        if vp.focus_nodes(ids):
            vp.selection.set_nodes(ids)
            vp.show_comment_pins = True
            self.app.selection_changed(None)
            self.app.status('Fit annotation in view: ' + ', '.join(self.app.doc.nodes[i].name for i in ids))
        else:
            self.app.status('This annotation has no geometry to frame')

    def reattach(self):
        if self.current_id(): self.app.set_tool(AnnotateTool(self.app.ctx, self.current_id()))

    def resolve(self):
        tid = self.current_id()
        if tid:
            status = "open" if self.app.doc.annotations[tid]["status"] == "resolved" else "resolved"
            self.app.ops.update_thread(tid, status=status)

    def edit_message(self):
        item = self.messages.currentItem()
        if not item or not self.current_id(): return
        c = next(c for c in self.app.doc.annotations[self.current_id()]["comments"] if c["id"] == item.data(Qt.UserRole))
        self.pending = None
        self.editing = c["id"]
        self.editor.setPlainText(c["body"])
        self.send.setText("Save edit")
        self.editor.setFocus()

    def delete_message(self):
        item = self.messages.currentItem()
        if item: self.app.ops.delete_comment(item.data(Qt.UserRole))

    def delete_thread(self):
        if self.current_id():
            self.app.ops.delete_thread(self.current_id())
            self.cancel_draft()

    def draw_pins(self, painter):
        self.pin_hits = []
        vp = self.app.viewport
        if not vp.show_comment_pins: return
        entries = [(t["id"], t["anchor"], str(k), t["anchor_status"]) for k,t in enumerate(self.cached, 1) if t["status"] == "open"]
        if self.pending: entries.append((None, self.pending, "+", "attached"))
        painter.setFont(QFont("Helvetica", 10, QFont.Bold))
        for tid, a, label, status in entries:
            if vp.inspection_ids is not None and tid != self.current_id(): continue
            if a['node_id'] is None or status == "missing" or not self.app.doc.is_visible(a["node_id"]): continue
            sp = vp.camera.project(vp.pose_point(a["node_id"], a["point"]), vp.width(), vp.height())
            if not sp or not 0 <= sp[0] <= vp.width() or not 0 <= sp[1] <= vp.height(): continue
            center = QPointF(sp[0] + 19, sp[1] - 19)
            color = QColor("#f6b957" if status == "needs_review" else "#74c8ef")
            painter.setPen(QPen(color, 2))
            painter.drawLine(QPointF(sp[0], sp[1]), center)
            painter.setBrush(QColor("#193749"))
            painter.drawEllipse(center, 13, 13)
            painter.setPen(QColor("#ffffff"))
            painter.drawText(QRectF(center.x()-13, center.y()-13, 26, 26), Qt.AlignCenter, label)
            if tid: self.pin_hits.append((center, tid))

    def pin_at(self, x, y):
        if not self.app.viewport.show_comment_pins: return None
        return next((tid for p,tid in reversed(self.pin_hits) if (p.x()-x)**2 + (p.y()-y)**2 <= 16**2), None)
