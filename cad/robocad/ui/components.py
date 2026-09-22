"""Reusable assemblies, typed parameter recipes and branch-local overrides."""
from copy import deepcopy
import json
from pathlib import Path

from PySide6.QtCore import Qt, QTimer
from PySide6.QtWidgets import (QWidget,QVBoxLayout,QHBoxLayout,QLabel,QPushButton,
    QListWidget,QListWidgetItem,QTabWidget,QTableWidget,QTableWidgetItem,QHeaderView,
    QFileDialog,QInputDialog,QDialog,QDialogButtonBox,QFormLayout,QLineEdit,
    QComboBox,QPlainTextEdit,QCheckBox,QProgressBar)

from ..component_jobs import ComponentJob
from ..component_parameters import FEATURES, validate_parameters
from ..kernel import KernelError
from ..document import Transform


def button(layout, text, action):
    widget=QPushButton(text); widget.clicked.connect(action); layout.addWidget(widget); return widget


class RecipeDialog(QDialog):
    def __init__(self, definition, parent):
        super().__init__(parent); self.setWindowTitle('Component parameters · '+definition.name); self.resize(800,560)
        layout=QVBoxLayout(self)
        label=QLabel('Shared defaults apply to every occurrence unless it has an override.\nDimensions use explicit units; expressions can refer to parameter names.'); label.setWordWrap(True); layout.addWidget(label)
        tabs=QTabWidget(); layout.addWidget(tabs)
        params=QWidget(); box=QVBoxLayout(params)
        self.table=QTableWidget(0,7); self.table.setHorizontalHeaderLabels(['Name','Default','Unit','Min','Max','Provenance','Description']); box.addWidget(self.table)
        self.table.horizontalHeader().setSectionResizeMode(QHeaderView.ResizeToContents)
        for name,spec in definition.parameters.items(): self.add_parameter(name,spec)
        row=QHBoxLayout(); box.addLayout(row)
        button(row,'Add parameter',lambda:self.add_parameter('',{'value':10,'unit':'mm','provenance':'estimated'}))
        button(row,'Remove selected',lambda:self.table.removeRow(self.table.currentRow()))
        tabs.addTab(params,'Parameters')
        advanced=QWidget(); box=QVBoxLayout(advanced)
        info=QLabel('Recipes: '+', '.join(FEATURES)+'. Fields accept values or parameter expressions. Assembly placement targets *.\nTargets in this definition:\n'+'\n'.join(f'{n.name}: {n.id}' for n in definition.nodes.values() if not n.component_member)); info.setWordWrap(True); box.addWidget(info)
        self.features=QPlainTextEdit(json.dumps(definition.features,indent=2)); box.addWidget(self.features); tabs.addTab(advanced,'Geometry and joints')
        childtab=QWidget(); box=QVBoxLayout(childtab)
        label=QLabel('Pass parent parameters to nested children. Example: {"length": "leg_length / 2"}.\nA local occurrence override takes precedence over this mapping.'); label.setWordWrap(True); box.addWidget(label)
        self.child_table=QTableWidget(0,3); self.child_table.setHorizontalHeaderLabels(['Child','Parameter bindings','Child defaults override']); box.addWidget(self.child_table)
        self.child_table.horizontalHeader().setSectionResizeMode(QHeaderView.Stretch)
        for nid,node in definition.nodes.items():
            if not node.component_instance or node.component_member: continue
            r=self.child_table.rowCount(); self.child_table.insertRow(r)
            item=QTableWidgetItem(node.name); item.setData(Qt.UserRole,nid); item.setFlags(item.flags() & ~Qt.ItemIsEditable); self.child_table.setItem(r,0,item)
            for col,key in ((1,'parameter_bindings'),(2,'overrides')):
                self.child_table.setItem(r,col,QTableWidgetItem(json.dumps(node.component_instance.get(key,{}))))
        tabs.addTab(childtab,'Nested parameters')
        self.family_table=QTableWidget(0,3)
        self.family_table.setHorizontalHeaderLabels(['Variant','Definition','Parameter bindings'])
        self.family_table.horizontalHeader().setSectionResizeMode(QHeaderView.Stretch)
        for name, variant in definition.variants.items():
            r=self.family_table.rowCount(); self.family_table.insertRow(r)
            for col,text in enumerate((name,variant['definition_id'],json.dumps(variant.get('parameter_bindings',{})))):
                item=QTableWidgetItem(text)
                if col<2:item.setFlags(item.flags() & ~Qt.ItemIsEditable)
                self.family_table.setItem(r,col,item)
        if definition.variants: tabs.addTab(self.family_table,'Family variants')
        self.error=QLabel(); self.error.setWordWrap(True); layout.addWidget(self.error)
        buttons=QDialogButtonBox(QDialogButtonBox.Ok|QDialogButtonBox.Cancel); buttons.accepted.connect(self.validate); buttons.rejected.connect(self.reject); layout.addWidget(buttons)
        self.original=definition.parameters

    def add_parameter(self,name,spec):
        r=self.table.rowCount(); self.table.insertRow(r)
        for col,value in enumerate((name,spec.get('value',''),spec.get('unit','mm'),spec.get('min',''),spec.get('max',''),spec.get('provenance','estimated'),spec.get('description',''))): self.table.setItem(r,col,QTableWidgetItem(str(value)))

    def validate(self):
        try:
            self.parameters={}
            for r in range(self.table.rowCount()):
                values=[self.table.item(r,c).text().strip() if self.table.item(r,c) else '' for c in range(7)]
                name,value,unit,low,high,provenance,description=values
                if name in self.parameters: raise ValueError('Parameter names must be unique')
                spec=deepcopy(self.original.get(name,{})); spec.update(value=value,unit=unit,provenance=provenance,description=description)
                for key,val in (('min',low),('max',high)):
                    if val: spec[key]=val
                    else: spec.pop(key,None)
                self.parameters[name]=spec
            from ..component_parameters import FEATURES, validate_parameters
            validate_parameters(self.parameters)
            self.recipe=json.loads(self.features.toPlainText()); self.nested={}
            if not isinstance(self.recipe,list): raise ValueError('Geometry recipe must be a list')
            for r in range(self.child_table.rowCount()):
                self.nested[self.child_table.item(r,0).data(Qt.UserRole)]={
                    'parameter_bindings':json.loads(self.child_table.item(r,1).text()),
                    'overrides':json.loads(self.child_table.item(r,2).text())}
            self.family_variants={self.family_table.item(r,0).text():{'definition_id':self.family_table.item(r,1).text(), 'parameter_bindings':json.loads(self.family_table.item(r,2).text())} for r in range(self.family_table.rowCount())} or None
            self.accept()
        except Exception as error: self.error.setText(str(error))


class ComponentsPanel(QWidget):
    def __init__(self, window):
        super().__init__(); self.window=window; self.jobs={}; self.current_instance=None
        self.library_path=Path.home()/'Documents'/'RoboCAD'/'Components'
        layout=QVBoxLayout(self)
        title=QLabel('Components'); title.setStyleSheet('font-size:18px;font-weight:600'); layout.addWidget(title)
        label=QLabel('Build once. Place linked assemblies. Override only what differs.'); label.setWordWrap(True); layout.addWidget(label)
        self.tabs=QTabWidget(); layout.addWidget(self.tabs)
        library=QWidget(); box=QVBoxLayout(library)
        box.addWidget(QLabel('In this document'))
        self.definition_search=QLineEdit(); self.definition_search.setPlaceholderText('Find a component…'); box.addWidget(self.definition_search)
        self.definitions=QListWidget(); box.addWidget(self.definitions)
        self.definition_search.textChanged.connect(self.filter_definitions)
        row=QHBoxLayout(); box.addLayout(row)
        button(row,'Make from selection',self.capture); button(row,'New parametric…',self.new)
        row=QHBoxLayout(); box.addLayout(row)
        button(row,'Place…',self.place); button(row,'Edit defaults…',self.edit_defaults)
        row=QHBoxLayout(); box.addLayout(row)
        button(row,'Import…',self.import_file); button(row,'Save to library…',self.export_file)
        box.addWidget(QLabel('Saved library'))
        self.files=QListWidget(); box.addWidget(self.files); self.files.itemDoubleClicked.connect(lambda *_:self.import_selected())
        row=QHBoxLayout(); box.addLayout(row)
        button(row,'Choose folder…',self.folder); button(row,'Import selected',self.import_selected)
        self.tabs.addTab(library,'Library')
        occurrence=QWidget(); box=QVBoxLayout(occurrence)
        self.instance_label=QLabel('Select a linked assembly or one of its parts.'); self.instance_label.setWordWrap(True); box.addWidget(self.instance_label)
        self.overrides=QTableWidget(0,4); self.overrides.setHorizontalHeaderLabels(['Parameter','Current','Override','Value']); self.overrides.horizontalHeader().setSectionResizeMode(QHeaderView.Stretch); box.addWidget(self.overrides)
        self.placement=QLineEdit(); self.placement.setPlaceholderText('X, Y, Z in mm'); box.addWidget(QLabel('Occurrence origin (mm)')); box.addWidget(self.placement)
        row=QHBoxLayout(); box.addLayout(row)
        button(row,'Apply occurrence',self.apply_override); button(row,'Reset to inherited',self.reset_override)
        button(box,'Detach outer occurrence',self.detach)
        self.tabs.addTab(occurrence,'Occurrence')
        self.progress=QProgressBar(); self.progress.setMinimumHeight(20)
        self.progress.setStyleSheet('QProgressBar {border:1px solid #526477; border-radius:3px; background:#171d25; text-align:center; color:white;} QProgressBar::chunk {background:#267fa5;}')
        self.progress.hide(); layout.addWidget(self.progress)
        self.status=QLabel(''); self.status.setWordWrap(True); layout.addWidget(self.status)
        self.cancel=button(layout,'Cancel rebuild',self.cancel_jobs); self.cancel.hide()
        self.timer=QTimer(self); self.timer.setInterval(32); self.timer.timeout.connect(self.poll)
        self.refresh()

    def filter_definitions(self):
        text=self.definition_search.text().casefold()
        for index in range(self.definitions.count()):
            item=self.definitions.item(index); item.setHidden(text not in item.text().casefold())

    def selected_definition(self):
        item=self.definitions.currentItem()
        if item is None: raise KernelError('Select a component in the library')
        return self.window.doc.component_definitions[item.data(Qt.UserRole)]

    def safe(self, fn):
        try: return fn()
        except Exception as error: self.status.setText(str(error)); self.window.error(str(error))

    def start(self, operation, args=(), kwargs=None):
        if any(j.state in ('pending','running','ready') for j in self.jobs.values()): raise KernelError('A component rebuild is already in progress')
        job=ComponentJob(self.window.doc,operation,args,kwargs)
        self.jobs[job.id]=job; job.start(); self.timer.start()
        self.progress.setRange(0,0); self.progress.show(); self.cancel.show()
        self.status.setText('Preparing component… You can keep viewing the model.')
        return job.status()

    def poll(self):
        active=False
        for job in list(self.jobs.values()):
            if job.state not in ('running','pending','ready'): continue
            job.poll()
            if job.state=='ready':
                job.commit(self.window.ops, self.window.viewport.items)
                if job.state=='applied':
                    self.status.setText('Saved to the component library.' if job.operation=='export_component' else 'Component updated. Undo restores the previous version.')
                    self.refresh()
                    result=job.result
                    if isinstance(result,str) and result in self.window.doc.component_definitions:
                        for i in range(self.definitions.count()):
                            if self.definitions.item(i).data(Qt.UserRole)==result: self.definitions.setCurrentRow(i)
                    target=result.get('instance_id') if isinstance(result,dict) else result
                    if isinstance(target,str) and target in self.window.doc.nodes:
                        self.window.viewport.selection.items=[(target,'body',0)]; self.window.selection_changed(None)
            if job.state=='failed': self.status.setText(job.error or 'Component rebuild failed')
            elif job.state=='cancelled': self.status.setText('Rebuild cancelled. Model unchanged.')
            elif job.state in ('running','pending'):
                active=True
                self.progress.setRange(0,job.total or 0); self.progress.setValue(job.done)
                self.status.setText(f'{job.stage} · {job.done}/{job.total}' if job.total else job.stage)
        if not active:
            self.timer.stop(); self.progress.hide(); self.cancel.hide()

    def cancel_jobs(self):
        for job in self.jobs.values(): job.cancel()

    def refresh(self):
        selected=self.definitions.currentItem(); key=selected.data(Qt.UserRole) if selected else next((n.component_instance['definition_id'] for n in self.window.doc.nodes.values() if n.component_instance and not n.component_member),None)
        self.definitions.clear()
        for d in self.window.doc.component_definitions.values():
            count=sum(bool(n.component_instance and n.component_instance['definition_id']==d.id) for n in self.window.doc.nodes.values())
            item=QListWidgetItem(f'{d.name}  ·  r{d.revision}  ·  {count} placed'); item.setData(Qt.UserRole,d.id); self.definitions.addItem(item)
            if d.id==key: self.definitions.setCurrentItem(item); self.definitions.scrollToItem(item)
        if not self.definitions.currentItem() and self.definitions.count(): self.definitions.setCurrentRow(0)
        self.files.clear()
        if self.library_path.exists():
            for path in sorted(self.library_path.glob('*.rcomp')):
                item=QListWidgetItem(path.stem); item.setData(Qt.UserRole,str(path)); self.files.addItem(item)
        self.filter_definitions()
        self.selection_changed()

    def selection_changed(self):
        doc=self.window.doc; selected=self.window.viewport.selection.nodes()
        node=doc.nodes.get(selected[0]) if selected else None
        if node and not node.component_instance and node.component_member: node=doc.nodes.get(node.component_member['instance_id'])
        self.current_instance=node.id if node and node.component_instance else None
        self.overrides.setRowCount(0)
        if self.current_instance is None: self.instance_label.setText('Select a linked assembly or one of its parts.'); self.placement.clear(); return
        spec=node.component_instance; definition=doc.component_definitions[spec['definition_id']]
        self.instance_label.setText(f'{node.name}\nLinked to {definition.name} · revision {definition.revision}'+(f'\nVariant: {spec["variant"]}' if spec.get('variant') else ''))
        local=spec.get('overrides',{})
        current=validate_parameters(definition.parameters, spec.get('overrides',{}))[0]
        if node.component_member:
            outer=node
            while outer.component_member: outer=doc.nodes[outer.component_member['instance_id']]
            source=next(k for k,v in outer.component_instance['node_map'].items() if v==node.id)
            local=outer.component_instance.get('nested_overrides',{}).get(source,{})
        for name,p in definition.parameters.items():
            r=self.overrides.rowCount(); self.overrides.insertRow(r)
            for col,text in ((0,name),(1,f'{current[name]:g} {p["unit"]}')):
                item=QTableWidgetItem(text); item.setFlags(item.flags() & ~Qt.ItemIsEditable); self.overrides.setItem(r,col,item)
            check=QCheckBox(); check.setChecked(name in local); self.overrides.setCellWidget(r,2,check)
            self.overrides.setItem(r,3,QTableWidgetItem(str(local.get(name,current[name]))))
        self.placement.setText(', '.join(f'{v:g}' for v in spec['placement']['translation']))
        self.placement.setEnabled(not bool(node.component_member))

    def capture(self):
        name,ok=QInputDialog.getText(self,'Make component','Component name')
        if ok and name.strip(): self.safe(lambda:self.start('make_component',[self.window.viewport.selection.nodes(),name.strip()]))

    def new(self):
        shape,ok=QInputDialog.getItem(self,'New parametric component','Shape',['box','cylinder'],0,False)
        if ok: self.safe(lambda:self.start('new_parametric_component',kwargs={'shape':shape,'name':'Parametric '+shape}))

    def place(self):
        def run():
            definition=self.selected_definition(); dialog=QDialog(self); dialog.setWindowTitle('Place '+definition.name); layout=QFormLayout(dialog)
            name=QLineEdit(definition.name); origin=QLineEdit('0, 0, 0'); angle=QLineEdit('0'); layout.addRow('Name',name); layout.addRow('Origin X, Y, Z (mm)',origin); layout.addRow('Rotation around Z (degrees)',angle)
            variants=QComboBox()
            if definition.variants:
                variants.addItems(list(definition.variants)); variants.setCurrentText(definition.default_variant); layout.addRow('Variant',variants)
            ports={}
            for key,port in definition.ports.items():
                combo=QComboBox()
                for n in self.window.doc.nodes.values():
                    if n.kind==port['kind']: combo.addItem(n.name,n.id)
                layout.addRow(port['label'],combo); ports[key]=combo
            buttons=QDialogButtonBox(QDialogButtonBox.Ok|QDialogButtonBox.Cancel); buttons.accepted.connect(dialog.accept); buttons.rejected.connect(dialog.reject); layout.addRow(buttons)
            if dialog.exec()!=QDialog.Accepted: return
            xyz=tuple(float(v.strip()) for v in origin.text().split(','))
            self.start('place_component',kwargs={'definition_id':definition.id,'placement':Transform(xyz,(0,0,1),float(angle.text())).to_json(),'bindings':{key:combo.currentData() for key,combo in ports.items()},'name':name.text(),'variant':variants.currentText() if definition.variants else None})
        self.safe(run)

    def edit_defaults(self):
        def run():
            definition=self.selected_definition(); dialog=RecipeDialog(definition,self)
            if dialog.exec()==QDialog.Accepted: self.start('set_component_parameters',[definition.id,dialog.parameters],{'features':dialog.recipe,'nested':dialog.nested,'family_variants':dialog.family_variants})
        self.safe(run)

    def apply_override(self):
        def run():
            if not self.current_instance: raise KernelError('Select an occurrence')
            values={self.overrides.item(r,0).text():self.overrides.item(r,3).text() for r in range(self.overrides.rowCount()) if self.overrides.cellWidget(r,2).isChecked()}
            node=self.window.doc.nodes[self.current_instance]; kwargs={}
            if not node.component_member:
                placement=deepcopy(node.component_instance['placement']); placement['translation']=[float(v.strip()) for v in self.placement.text().split(',')]; kwargs['placement']=placement
            self.start('set_component_overrides',[self.current_instance,values],kwargs)
        self.safe(run)

    def reset_override(self):
        if self.current_instance: self.safe(lambda:self.start('set_component_overrides',[self.current_instance,{}]))

    def detach(self):
        def run():
            if not self.current_instance: raise KernelError('Select an occurrence')
            node=self.window.doc.nodes[self.current_instance]
            while node.component_member: node=self.window.doc.nodes[node.component_member['instance_id']]
            self.start('detach_component',[node.id])
        self.safe(run)

    def import_file(self):
        path,_=QFileDialog.getOpenFileName(self,'Import component library',str(self.library_path),'Components (*.rcomp)')
        if path:self.safe(lambda:self.start('import_component',[path]))

    def import_selected(self):
        item=self.files.currentItem()
        if item:self.safe(lambda:self.start('import_component',[item.data(Qt.UserRole)]))

    def export_file(self):
        def run():
            definition=self.selected_definition(); self.library_path.mkdir(parents=True,exist_ok=True)
            path,_=QFileDialog.getSaveFileName(self,'Save component library',str(self.library_path/(definition.name+'.rcomp')),'Components (*.rcomp)')
            if path:self.start('export_component',[definition.id,path if path.endswith('.rcomp') else path+'.rcomp'])
        self.safe(run)

    def folder(self):
        path=QFileDialog.getExistingDirectory(self,'Component library folder',str(self.library_path))
        if path:self.library_path=Path(path); self.refresh()
