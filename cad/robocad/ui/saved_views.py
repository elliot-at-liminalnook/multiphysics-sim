"""A lightweight library of labeled cameras and cutaways."""
from PySide6.QtCore import Qt
from PySide6.QtWidgets import (QHBoxLayout, QInputDialog, QLabel, QLineEdit,
    QListWidget, QListWidgetItem, QPushButton, QVBoxLayout, QWidget)

from ..saved_views import capture_view, restore_view


class SavedViewsPanel(QWidget):
    def __init__(self, app):
        super().__init__(app)
        self.app = app
        layout = QVBoxLayout(self)
        hint = QLabel('Save camera angles and cutaways with a name.\nDouble-click a view to return to it.')
        hint.setWordWrap(True)
        layout.addWidget(hint)
        row = QHBoxLayout()
        self.name = QLineEdit()
        self.name.setPlaceholderText('View name, e.g. Worm drive cutaway')
        self.name.setMaxLength(120)
        self.name.setAccessibleName('New saved view name')
        row.addWidget(self.name)
        self.save_button = QPushButton('Save current view')
        self.save_button.setObjectName('primaryAction')
        self.save_button.clicked.connect(lambda: app._safe(self.save_current))
        self.name.returnPressed.connect(lambda: app._safe(self.save_current))
        self.name.textChanged.connect(self.update_actions)
        row.addWidget(self.save_button)
        layout.addLayout(row)
        self.views = QListWidget()
        self.views.setWordWrap(True)
        self.views.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.views.currentItemChanged.connect(self.update_actions)
        self.views.itemDoubleClicked.connect(lambda *_: app._safe(self.restore))
        layout.addWidget(self.views)
        self.empty = QLabel('No saved views yet. Position the model, enter a name, then save.')
        self.empty.setWordWrap(True)
        layout.addWidget(self.empty)
        self.actions = []
        for entries in [ [('Restore view', self.restore), ('Replace with current', self.replace_current)],
                         [('Rename…', self.rename), ('Delete', self.delete)] ]:
            row = QHBoxLayout()
            for title, callback in entries:
                button = QPushButton(title)
                button.clicked.connect(lambda checked=False, cb=callback: app._safe(cb))
                row.addWidget(button)
                self.actions.append(button)
            layout.addLayout(row)
        self.actions[1].setToolTip('Update the selected view to this camera and cutaway; Undo restores the previous view')
        self.actions[3].setToolTip('Remove the selected saved view; Undo restores it')
        self.feedback = QLabel('Saved inside this CAD file · edits support Undo')
        self.feedback.setWordWrap(True)
        layout.addWidget(self.feedback)
        self.refresh()

    def current_id(self):
        item = self.views.currentItem()
        return item.data(Qt.UserRole) if item else None

    def update_actions(self, *_):
        for button in getattr(self, 'actions', []):
            button.setEnabled(self.current_id() is not None)
        self.save_button.setEnabled(bool(self.name.text().strip()))

    def refresh(self):
        selected = self.current_id()
        self.views.clear()
        for view in self.app.ops.saved_views():
            state = view['state']
            details = 'Orthographic' if state['orthographic'] else 'Perspective'
            if state['section']['enabled']: details += ' · Cutaway'
            item = QListWidgetItem(view['name'] + '\n' + details)
            item.setData(Qt.UserRole, view['id'])
            item.setToolTip(view['name'] + '\nDouble-click to restore camera and cutaway')
            self.views.addItem(item)
            if view['id'] == selected: self.views.setCurrentItem(item)
        self.empty.setVisible(self.views.count() == 0)
        self.update_actions()

    def select(self, vid):
        for i in range(self.views.count()):
            item = self.views.item(i)
            if item.data(Qt.UserRole) == vid: self.views.setCurrentItem(item)

    def save_current(self):
        vid = self.app.ops.save_view(self.name.text(), capture_view(self.app.viewport))
        self.name.clear()
        self.refresh()
        self.select(vid)
        self.feedback.setText('Saved: ' + self.app.doc.saved_views[vid]['name'])

    def restore(self):
        vid = self.current_id()
        if vid is None: return
        view = self.app.doc.saved_views[vid]
        self.app.comments.end_inspection()
        restore_view(self.app, view['state'])
        self.feedback.setText('Showing: ' + view['name'])
        self.app.status('Restored view: ' + view['name'])

    def replace_current(self):
        vid = self.current_id()
        if vid is None: return
        self.app.ops.update_saved_view(vid, state=capture_view(self.app.viewport))
        self.feedback.setText('Updated: ' + self.app.doc.saved_views[vid]['name'] + ' · Undo to revert')

    def rename(self):
        vid = self.current_id()
        if vid is None: return
        name, ok = QInputDialog.getText(self, 'Rename saved view', 'View name:', text=self.app.doc.saved_views[vid]['name'])
        if ok:
            self.app.ops.update_saved_view(vid, name=name)

    def delete(self):
        vid = self.current_id()
        if vid is None: return
        self.app.ops.delete_saved_view(vid)
        self.feedback.setText('View deleted · Undo to restore')
