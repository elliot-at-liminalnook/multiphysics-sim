"""Registry-driven component inspector and interactive connection view."""
from PySide6.QtCore import Qt, Signal, QPointF, QSize
from PySide6.QtGui import QColor, QPen, QBrush, QPainterPath, QPainter, QFontMetricsF
from PySide6.QtWidgets import (QWidget, QVBoxLayout, QHBoxLayout, QFormLayout,
    QComboBox, QLineEdit, QPushButton, QLabel, QTableWidget, QTableWidgetItem,
    QHeaderView, QSplitter, QGraphicsView, QGraphicsScene, QScrollArea, QFrame)
from ..component_graph import RegistryView, edit_graph
from ..component_derivation import RECIPES
from ..kernel import KernelError


class ConnectionsView(QGraphicsView):
    component_selected = Signal(str)
    port_selected = Signal(object)
    connection_selected = Signal(str)

    def sizeHint(self):
        # Scene size is independent of the dock's preferred on-screen size.
        return QSize(460, 240)

    def __init__(self):
        super().__init__()
        self.setScene(QGraphicsScene(self))
        self.cards = {}
        self.setDragMode(QGraphicsView.ScrollHandDrag)
        self.setRenderHint(QPainter.Antialiasing)
        self.setTransformationAnchor(QGraphicsView.AnchorUnderMouse)
        self.setMinimumHeight(160)
        self.setToolTip('Click a component to inspect it. Click two ports to connect them. Scroll to zoom; drag the background to pan.')

    def zoom(self, factor):
        scale = self.transform().m11()
        target = min(3., max(.15, scale*factor))
        self.scale(target/scale, target/scale)

    def wheelEvent(self, event):
        delta = event.angleDelta().y()
        if delta: self.zoom(1.2 if delta > 0 else 1/1.2)
        event.accept()

    def focus_component(self, identity):
        card = self.cards.get(identity)
        if card is not None:
            bounds = card.sceneBoundingRect().adjusted(-12, -12, 12, 12)
            self.fitInView(bounds, Qt.KeepAspectRatio)
            if self.transform().m11() > 1.: self.resetTransform()
            self.centerOn(bounds.center())

    def show_graph(self, graph, registry, selected=None):
        scene = self.scene(); scene.clear(); self.cards = {}
        endpoints = {}; tops = {}
        heights = [0., 0.]
        for index, (identity, component) in enumerate(graph['components'].items()):
            column = index % 2; x, y = column*340., heights[column]
            try: ports = registry.ports(component)
            except KernelError: ports = []
            height = 62+24*len(ports); heights[column] += height+45
            tops[identity] = y
            card = scene.addRect(x, y, 270, height, QPen(QColor('#69bfff' if identity == selected else '#748393')),
                                 QBrush(QColor('#28313c')))
            card.setData(0, ('component', identity)); card.setCursor(Qt.PointingHandCursor)
            self.cards[identity] = card
            title = scene.addText(''); title.setDefaultTextColor(QColor('#ffffff'))
            title.setPlainText(QFontMetricsF(title.font()).elidedText(component['name'], Qt.ElideRight, 248))
            title.setToolTip(component['name'])
            title.setPos(x+8, y+3); title.setData(0, ('component', identity)); title.setCursor(Qt.PointingHandCursor)
            subtitle = scene.addText(component['type']); subtitle.setDefaultTextColor(QColor('#b8c8d8'))
            subtitle.setPlainText(QFontMetricsF(subtitle.font()).elidedText(component['type'], Qt.ElideRight, 248))
            subtitle.setToolTip(component['type']); subtitle.setCursor(Qt.PointingHandCursor)
            subtitle.setPos(x+8, y+26); subtitle.setData(0, ('component', identity))
            for i, port in enumerate(ports):
                py = y+64+24*i
                endpoint = {'component_id': identity, 'port': port['name']}
                label = scene.addText(''); label.setDefaultTextColor(QColor('#e0e9f0'))
                label.setPlainText(QFontMetricsF(label.font()).elidedText(port['name'], Qt.ElideRight, 228))
                label.setPos(x+12, py-13); label.setData(0, ('port', endpoint)); label.setCursor(Qt.CrossCursor)
                dot = scene.addEllipse(x+255, py-5, 10, 10, QPen(QColor('#79ccae')), QBrush(QColor('#79ccae')))
                dot.setData(0, ('port', endpoint)); dot.setCursor(Qt.CrossCursor)
                units = port.get('unit') or ', '.join(f"{l['across_unit']} / {l['through_unit']}" for l in port.get('lanes', []))
                label.setToolTip(f"{port['name']} · {port.get('direction', '')} · {units}"); dot.setToolTip(label.toolTip())
                endpoints[(identity, port['name'])] = QPointF(x+260, py)
        for identity, connection in graph['connections'].items():
            points = [endpoints.get((p['component_id'], p['port'])) for p in connection['ports']]
            points = [p for p in points if p is not None]
            if not points: continue
            for point in points[1:] or [points[0]+QPointF(28, 0)]:
                path = QPainterPath(points[0])
                if len(points) == 1: path.lineTo(point)
                else:
                    top = min(tops[p['component_id']] for p in connection['ports'] if p['component_id'] in tops)-18
                    path.lineTo(points[0]+QPointF(30, 0)); path.lineTo(QPointF(points[0].x()+30, top))
                    path.lineTo(QPointF(point.x()+30, top)); path.lineTo(point+QPointF(30, 0)); path.lineTo(point)
                wire = scene.addPath(path, QPen(QColor('#e7ba69'), 3))
                wire.setZValue(-1); wire.setData(0, ('connection', identity)); wire.setCursor(Qt.PointingHandCursor)
                wire.setToolTip('Connection · click to select for removal')
        scene.setSceneRect(scene.itemsBoundingRect().adjusted(-20, -20, 60, 20))

    def mousePressEvent(self, event):
        item = self.itemAt(event.position().toPoint())
        payload = item.data(0) if item else None
        if event.button() == Qt.LeftButton and payload:
            kind, value = payload
            {'component': self.component_selected, 'port': self.port_selected,
             'connection': self.connection_selected}[kind].emit(value)
            event.accept(); return
        super().mousePressEvent(event)

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape:
            self.port_selected.emit(None); event.accept(); return
        super().keyPressEvent(event)


class SystemGraphPanel(QWidget):
    document_event = Signal()

    def __init__(self, app):
        super().__init__()
        self.app = app
        self.catalogue = []; self.imported = []; self.import_revision = None; self.registry = RegistryView([])
        self.import_state = None; self.imports_stale = False
        self.checked_components = {}
        self.current = None; self.pending_port = None; self.selected_connection = None
        self.edit_revision = app.doc.revision
        layout = QVBoxLayout(self); layout.setContentsMargins(2, 4, 2, 2)
        self.help = QLabel('Load the Rust catalogue in Components, then add a system component.'); self.help.setWordWrap(True)
        layout.addWidget(self.help)
        row = QHBoxLayout(); self.types = QComboBox(); self.types.setMinimumWidth(100)
        self.types.setSizeAdjustPolicy(QComboBox.AdjustToMinimumContentsLengthWithIcon); self.types.setMinimumContentsLength(18)
        row.addWidget(self.types, 1)
        self.add_button = QPushButton('New'); self.add_button.clicked.connect(self.new_component); row.addWidget(self.add_button)
        layout.addLayout(row)
        import_row = QHBoxLayout()
        self.imports = QComboBox(); self.imports.setMinimumContentsLength(15)
        self.imports.setSizeAdjustPolicy(QComboBox.AdjustToMinimumContentsLengthWithIcon)
        self.imports.addItem('Check system to discover CAD components', None); import_row.addWidget(self.imports, 1)
        self.use_import = QPushButton('Use existing'); self.use_import.setEnabled(False)
        self.use_import.clicked.connect(lambda: self.safe(self.use_imported)); import_row.addWidget(self.use_import)
        check = QPushButton('Check'); check.clicked.connect(lambda: self.app._safe(self.app.experiments_panel.check_system)); import_row.addWidget(check)
        layout.addLayout(import_row)
        self.import_status = QLabel(''); self.import_status.setWordWrap(True); layout.addWidget(self.import_status)
        splitter = QSplitter(Qt.Vertical); splitter.setHandleWidth(7); layout.addWidget(splitter, 1)
        upper = QWidget(); form_layout = QVBoxLayout(upper); form_layout.setContentsMargins(0, 0, 0, 0)
        self.components = QComboBox(); self.components.currentIndexChanged.connect(self.select_component); form_layout.addWidget(self.components)
        self.components.setSizeAdjustPolicy(QComboBox.AdjustToMinimumContentsLengthWithIcon); self.components.setMinimumContentsLength(18)
        form = QFormLayout(); form.setFieldGrowthPolicy(QFormLayout.AllNonFixedFieldsGrow)
        form.setFormAlignment(Qt.AlignTop | Qt.AlignLeft)
        self.name = QLineEdit(); form.addRow('Name', self.name)
        self.body = QComboBox(); form.addRow('CAD body', self.body)
        self.binding = QLineEdit(); self.binding.setPlaceholderText('New component (leave blank)')
        self.binding.textChanged.connect(lambda: self.refresh_checked_values() if hasattr(self, 'parameters') else None)
        self.binding.setToolTip('Existing imported component name, for example drive1.case. Applies explicit parameter overrides and exposes its ports without creating another component. Checked when building the captured run.')
        form.addRow('Bind existing', self.binding)
        self.derivation = QComboBox(); self.derivation.currentIndexChanged.connect(self.derivation_changed)
        form.addRow('Geometry rule', self.derivation)
        self.specific_heat = QLineEdit(); self.specific_heat.setPlaceholderText('Use material specific heat')
        self.specific_heat.setToolTip('Optional specific heat override in J/(kg·K); empty uses the attached body material.')
        form.addRow('Specific heat · J/(kg·K)', self.specific_heat)
        self.flow_direction = QComboBox(); self.flow_direction.addItem('a → b along cylinder axis', 1); self.flow_direction.addItem('a → b against cylinder axis', -1)
        form.addRow('Fluid direction', self.flow_direction)
        self.recipe_form = form
        form_layout.addLayout(form)
        attach = QPushButton('Attach to selected CAD body'); attach.clicked.connect(self.attach_selected); form_layout.addWidget(attach)
        self.parameters = QTableWidget(0, 4); self.parameters.setHorizontalHeaderLabels(['Parameter', 'Value', 'Unit', 'Last check'])
        self.parameters.itemChanged.connect(self.parameter_changed)
        self.parameters.horizontalHeader().setSectionResizeMode(0, QHeaderView.Stretch)
        self.parameters.horizontalHeader().setSectionResizeMode(1, QHeaderView.ResizeToContents)
        self.parameters.horizontalHeader().setSectionResizeMode(2, QHeaderView.ResizeToContents)
        self.parameters.horizontalHeader().setSectionResizeMode(3, QHeaderView.ResizeToContents)
        self.parameters.setMinimumHeight(120); form_layout.addWidget(self.parameters, 1)
        row = QHBoxLayout()
        extra = QPushButton('+ Parameter'); extra.clicked.connect(lambda: self.parameter_row('', '', '')); row.addWidget(extra)
        self.apply_button = QPushButton('Apply'); self.apply_button.clicked.connect(lambda: self.safe(self.apply)); row.addWidget(self.apply_button)
        self.remove_button = QPushButton('Remove'); self.remove_button.clicked.connect(lambda: self.safe(self.remove)); row.addWidget(self.remove_button)
        form_layout.addLayout(row)
        self.inspector_scroll = QScrollArea(); self.inspector_scroll.setWidgetResizable(True)
        self.inspector_scroll.setFrameShape(QFrame.NoFrame); self.inspector_scroll.setWidget(upper)
        self.inspector_scroll.setMinimumHeight(100)
        splitter.addWidget(self.inspector_scroll)
        lower = QWidget(); lower_layout = QVBoxLayout(lower); lower_layout.setContentsMargins(0, 0, 0, 0)
        graph_tools = QHBoxLayout()
        fit = QPushButton('Overview'); fit.clicked.connect(lambda: self.view.fitInView(self.view.sceneRect(), Qt.KeepAspectRatio))
        fit.setToolTip('Fit the entire connection graph; scroll to zoom into details.'); graph_tools.addWidget(fit)
        focus = QPushButton('Focus selected'); focus.clicked.connect(lambda: self.view.focus_component(self.current)); graph_tools.addWidget(focus)
        for label, factor in [('−', 1/1.2), ('+', 1.2)]:
            button = QPushButton(label); button.setMaximumWidth(32)
            button.clicked.connect(lambda checked=False, factor=factor: self.view.zoom(factor)); graph_tools.addWidget(button)
        lower_layout.addLayout(graph_tools)
        self.view = ConnectionsView(); lower_layout.addWidget(self.view, 1)
        self.view.component_selected.connect(self.choose_component)
        self.view.port_selected.connect(lambda endpoint: self.safe(lambda: self.choose_port(endpoint)))
        self.view.connection_selected.connect(self.choose_connection)
        row = QHBoxLayout(); self.open_button = QPushButton('Leave port open'); self.open_button.clicked.connect(lambda: self.safe(self.open_port)); row.addWidget(self.open_button)
        self.disconnect_button = QPushButton('Remove connection'); self.disconnect_button.clicked.connect(lambda: self.safe(self.disconnect)); row.addWidget(self.disconnect_button)
        lower_layout.addLayout(row); splitter.addWidget(lower)
        splitter.handle(1).setToolTip('Drag to resize the inspector and connection view')
        self.status = QLabel('Ready'); self.status.setWordWrap(True); layout.addWidget(self.status)
        self.document_event.connect(self.refresh)
        app.doc.listeners.append(self.document_changed)
        self.refresh()

    def safe(self, function):
        try: function()
        except (KernelError, KeyError, ValueError, TypeError) as error:
            self.status.setText(str(error)); self.status.setStyleSheet('color: #e9a15e')

    def receive_catalogue(self, catalogue):
        self.catalogue = catalogue; self.registry = RegistryView(catalogue, self.imported)
        self.types.clear()
        for entry in catalogue: self.types.addItem(entry['type'], entry['type'])
        self.help.setText('Select a component to edit it. Blank values keep imported values or use defaults. Click two ports to connect; Escape cancels.')
        self.refresh()

    def receive_imports(self, result):
        self.imported = result['imported']; self.import_revision = result['revision']
        self.import_state = result['state']; self.imports_stale = result.get('stale', False)
        self.registry = RegistryView(self.catalogue, self.imported)
        self.checked_components = {key: entry for entry in result.get('resolved') or [] for key in (entry['binding'], entry['name'])}
        self.imports.clear()
        for entry in self.imported: self.imports.addItem(f"{entry['name']} · {entry['type']}", entry)
        self.use_import.setEnabled(bool(self.imported))
        self.refresh()
        self.refresh_checked_values()
        if result.get('error'): self.status.setText(result['error'])

    def mark_imports_stale(self):
        self.imports_stale = True
        self.refresh_import_status()

    def refresh_import_status(self):
        if self.import_revision is not None:
            state = ' · document has changed; check again for current values' if self.app.doc.revision != self.import_revision else (
                ' · scripts or settings changed; check again for current values' if self.imports_stale else '')
            if self.import_state != 'completed': state += f' · system {self.import_state}'
            self.import_status.setText(f'Imported components from captured revision {self.import_revision}{state}')

    def refresh_checked_values(self):
        identity = self.binding.text().strip() or ('graph/'+self.current if self.current else None)
        values = self.checked_components.get(identity, {}).get('parameters', {})
        for row in range(self.parameters.rowCount()):
            key = self.parameters.item(row, 0).text()
            value = values.get(key)
            item = QTableWidgetItem(format(value, '.8g') if isinstance(value, (int, float)) else (str(value) if value is not None else ''))
            item.setFlags(item.flags() & ~Qt.ItemIsEditable)
            item.setToolTip('Parameter value from the last native build. Apply edits and check again to update it.')
            self.parameters.setItem(row, 3, item)

    def use_imported(self):
        entry = self.imports.currentData()
        if not entry: return
        if entry['type'] not in self.registry.types: raise KernelError('Load the Rust catalogue before using an imported component')
        for identity, component in self.app.doc.component_graph['components'].items():
            if component.get('binding') in (entry['binding'], entry['name']):
                self.choose_component(identity); return
        self.current = None; self.components.setCurrentIndex(-1)
        self.fill({'name': entry['name'], 'type': entry['type'], 'body_id': entry.get('body_id'),
                   'binding': entry['binding'], 'parameters': {}})
        self.status.setText('Existing CAD component · blank values retain captured parameters. Apply adds its binding to the graph.')

    def document_changed(self, event, payload):
        if event == 'changed': self.document_event.emit()

    def refresh(self):
        self.refresh_import_status()
        current = self.current
        self.components.blockSignals(True); self.components.clear()
        for identity, component in self.app.doc.component_graph['components'].items():
            self.components.addItem(component['name'], identity)
        self.components.setCurrentIndex(self.components.findData(current))
        self.components.blockSignals(False)
        # Preserve unsaved inspector values across external edits. Apply checks
        # the revision captured when this form was loaded.
        if current not in self.app.doc.component_graph['components']:
            self.current = None
        self.view.show_graph(self.app.doc.component_graph, self.registry, self.current)
        self.add_button.setEnabled(bool(self.catalogue))
        self.remove_button.setEnabled(self.current is not None)
        self.disconnect_button.setEnabled(self.selected_connection in self.app.doc.component_graph['connections'])
        self.open_button.setEnabled(self.pending_port is not None)

    def fill(self, component):
        self.edit_revision = self.app.doc.revision
        self.types.setCurrentIndex(self.types.findData(component.get('type')))
        self.name.setText(component.get('name', ''))
        self.binding.setText(component.get('binding') or '')
        self.body.clear(); self.body.addItem('No body attachment', None)
        for node in self.app.doc.bodies(): self.body.addItem(node.name, node.id)
        self.body.setCurrentIndex(max(0, self.body.findData(component.get('body_id'))))
        self.parameters.setRowCount(0)
        self.draft_type = component.get('type')
        descriptor = self.registry.types.get(self.draft_type, {})
        values = component.get('parameters', {})
        inherited = self.registry.imported.get(component.get('binding'), {}).get('parameters', {})
        declared = set()
        for parameter in descriptor.get('parameters') or []:
            name = parameter['name']
            if '*' in name: continue
            declared.add(name)
            default = parameter.get('default') if parameter.get('default') is not None else parameter.get('default_label')
            hint = 'Blank keeps the imported value' if component.get('binding') else ('Required' if parameter['required'] else f'Default: {default}')
            if name in inherited: hint += f" · captured value: {inherited[name]} {parameter['unit']}"
            if parameter.get('minimum') is not None: hint += f" · minimum {'>' if parameter.get('exclusive_minimum') else '≥'} {parameter['minimum']}"
            if parameter.get('maximum') is not None: hint += f" · maximum {parameter['maximum']}"
            if parameter.get('integer'): hint += ' · whole number'
            self.parameter_row(name, str(values[name]) if name in values else '', parameter['unit'], hint)
        for name, value in values.items():
            if name not in declared: self.parameter_row(name, str(value), '')
        self.draft_type = component.get('type')
        self.derivation.blockSignals(True); self.derivation.clear(); self.derivation.addItem('Enter parameters directly', None)
        if self.draft_type == 'thermal.capacitance': self.derivation.addItem('Body mass × specific heat', 'body_thermal_capacity')
        if self.draft_type == 'fluid.pipe_ph': self.derivation.addItem('Circular cylinder = fluid volume', 'circular_fluid_volume')
        recipe = component.get('derivation') or {}
        self.derivation.setCurrentIndex(max(0, self.derivation.findData(recipe.get('kind')))); self.derivation.blockSignals(False)
        self.specific_heat.setText(str(recipe['specific_heat']) if 'specific_heat' in recipe else '')
        self.flow_direction.setCurrentIndex(max(0, self.flow_direction.findData(recipe.get('flow_direction', 1))))
        self.derivation_changed()
        self.refresh_checked_values()
        self.status.setText('Values are in declared units. Apply saves one undoable edit.')
        self.status.setStyleSheet('')

    def parameter_row(self, name, value, unit, hint=''):
        self.parameters.blockSignals(True)
        row = self.parameters.rowCount(); self.parameters.insertRow(row)
        for column, text in enumerate((name, value, unit, '')):
            item = QTableWidgetItem(text); item.setToolTip(hint)
            if column in (2, 3) or (column == 0 and unit): item.setFlags(item.flags() & ~Qt.ItemIsEditable)
            self.parameters.setItem(row, column, item)
        self.parameters.blockSignals(False)
        if not unit: self.parameter_changed(self.parameters.item(row, 0))

    def parameter_changed(self, item):
        if item.column() != 0 or not getattr(self, 'draft_type', None): return
        name = item.text().strip()
        declaration = self.registry.parameter({'type': self.draft_type}, name)
        if declaration:
            unit = declaration['unit']
            default = declaration.get('default') if declaration.get('default') is not None else declaration.get('default_label')
            hint = declaration['name'] + (' · required' if declaration['required'] else f' · default: {default}')
            if declaration.get('integer'): hint += ' · whole number'
            if declaration.get('minimum') is not None:
                hint += f" · minimum {'>' if declaration.get('exclusive_minimum') else '≥'} {declaration['minimum']}"
            if declaration.get('maximum') is not None: hint += f" · maximum {declaration['maximum']}"
        else:
            unit = 'Unknown' if name else ''
            hint = 'Enter a declared parameter or family member, such as patch0.x0.'
        self.parameters.blockSignals(True)
        self.parameters.item(item.row(), 2).setText(unit)
        for column in range(4): self.parameters.item(item.row(), column).setToolTip(hint)
        self.parameters.blockSignals(False)
        self.refresh_checked_values()

    def new_component(self):
        kind = self.types.currentData()
        if not kind: return
        self.current = None; self.components.setCurrentIndex(-1)
        names = {c['name'] for c in self.app.doc.component_graph['components'].values()}
        name = kind; index = 2
        while name in names: name = f'{kind} {index}'; index += 1
        self.fill({'type': kind, 'name': name})
        self.remove_button.setEnabled(False)

    def select_component(self):
        identity = self.components.currentData()
        if not identity: return
        self.current = identity
        self.fill(self.app.doc.component_graph['components'][identity])
        self.view.show_graph(self.app.doc.component_graph, self.registry, identity)
        self.remove_button.setEnabled(True)

    def choose_component(self, identity):
        self.components.setCurrentIndex(self.components.findData(identity))
        self.select_component()
        component = self.app.doc.component_graph['components'][identity]
        if component.get('body_id') in self.app.doc.nodes:
            self.app.viewport.selection.set_nodes([component['body_id']]); self.app.selection_changed(None)

    def attach_selected(self):
        bodies = self.app.viewport.selection.nodes()
        if len(bodies) != 1 or self.body.findData(bodies[0]) < 0:
            self.status.setText('Select one CAD body in the viewport first.'); return
        self.body.setCurrentIndex(self.body.findData(bodies[0]))

    def publish(self, operation, revision=None):
        result = edit_graph(self.app.doc, self.app.ops, operation,
            self.app.doc.revision if revision is None else revision, self.catalogue)
        self.status.setStyleSheet(''); self.status.setText('Saved · undo is available')
        return result

    def apply(self):
        if not getattr(self, 'draft_type', None): raise KernelError('Choose a component type and click New first')
        parameters = {}
        kind = self.derivation.currentData()
        derived = RECIPES[kind]['outputs'] if kind else {}
        for row in range(self.parameters.rowCount()):
            key, value = [self.parameters.item(row, col).text().strip() for col in (0, 1)]
            if key in derived: continue
            if not value: continue
            if key in parameters: raise KernelError(f'Duplicate parameter {key}')
            try: parameters[key] = float(value)
            except ValueError: raise KernelError(f'{key} requires a number in the declared unit')
        recipe = {'kind': kind} if kind else None
        if kind == 'body_thermal_capacity' and self.specific_heat.text().strip(): recipe['specific_heat'] = float(self.specific_heat.text())
        if kind == 'circular_fluid_volume': recipe['flow_direction'] = self.flow_direction.currentData()
        result = self.publish({'action': 'update_component' if self.current else 'add_component',
            'id': self.current, 'component': {'name': self.name.text().strip(), 'type': self.draft_type,
                'body_id': self.body.currentData(), 'parameters': parameters, 'derivation': recipe,
                'binding': self.binding.text().strip() or None}}, self.edit_revision)
        self.current = result['id']; self.refresh(); self.choose_component(self.current)

    def remove(self):
        if self.current: self.publish({'action': 'delete_component', 'id': self.current}, self.edit_revision)
        self.current = None; self.parameters.setRowCount(0); self.name.clear(); self.refresh()

    def derivation_changed(self):
        kind = self.derivation.currentData()
        derived = RECIPES[kind]['outputs'] if kind else {}
        self.specific_heat.setEnabled(kind == 'body_thermal_capacity')
        self.flow_direction.setEnabled(kind == 'circular_fluid_volume')
        self.recipe_form.setRowVisible(self.specific_heat, kind == 'body_thermal_capacity')
        self.recipe_form.setRowVisible(self.flow_direction, kind == 'circular_fluid_volume')
        for row in range(self.parameters.rowCount()):
            key = self.parameters.item(row, 0).text()
            item = self.parameters.item(row, 1)
            if key in derived:
                item.setText('Derived from CAD'); item.setFlags(item.flags() & ~Qt.ItemIsEditable)
                item.setToolTip('Computed from captured geometry in the worker; see component_derivations.json for inputs and formula.')
            else:
                if item.text() == 'Derived from CAD': item.setText('')
                item.setFlags(item.flags() | Qt.ItemIsEditable)

    def choose_port(self, endpoint):
        if endpoint is None:
            self.pending_port = None; self.view.unsetCursor(); self.status.setText('Connection cancelled'); self.refresh(); return
        if self.pending_port is None:
            self.pending_port = endpoint; self.connection_revision = self.app.doc.revision
            self.view.setCursor(Qt.CrossCursor)
            self.status.setText(f"Connect {endpoint['port']}: click another port, or leave it open.")
            self.refresh(); return
        if endpoint == self.pending_port: raise KernelError('Choose another port, or use Leave port open')
        self.publish({'action': 'connect', 'ports': [self.pending_port, endpoint]}, self.connection_revision)
        self.pending_port = None; self.view.unsetCursor(); self.refresh()

    def open_port(self):
        if self.pending_port:
            if any(self.pending_port in c['ports'] for c in self.app.doc.component_graph['connections'].values()):
                raise KernelError('This port is already connected. Remove its connection first to declare it open.')
            self.publish({'action': 'connect', 'ports': [self.pending_port]}, self.connection_revision)
            self.pending_port = None; self.view.unsetCursor(); self.refresh()

    def choose_connection(self, identity):
        self.selected_connection = identity; self.connection_revision = self.app.doc.revision
        self.disconnect_button.setEnabled(True); self.status.setText('Connection selected · Remove connection disconnects its ports')
        for item in self.view.scene().items():
            if item.data(0) == ('connection', identity): item.setPen(QPen(QColor('#ffffff'), 4))

    def disconnect(self):
        if self.selected_connection:
            self.publish({'action': 'delete_connection', 'id': self.selected_connection}, self.connection_revision)
            self.selected_connection = None; self.refresh()

    def shutdown(self):
        self.app.doc.listeners.remove(self.document_changed)
