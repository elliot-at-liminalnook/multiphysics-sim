"""Persisted system composition, independent of viewport layout and run state."""
from copy import deepcopy
import json
import math
import uuid
from pathlib import PurePosixPath
from .kernel import KernelError


def empty_graph():
    return {'version': 1, 'components': {}, 'connections': {}}


def validate_graph(graph, doc):
    """Validate document identities before publishing an atomic graph edit.

    Native registry type/parameter/connection validation belongs to the composition
    service; this layer also runs when loading/staging document edits offline.
    """
    if not isinstance(graph, dict) or set(graph) != {'version', 'components', 'connections'}:
        raise KernelError('System graph requires version, components and connections')
    if type(graph['version']) is not int or graph['version'] != 1:
        raise KernelError('Unsupported system graph version')
    for section in ('components', 'connections'):
        if not isinstance(graph[section], dict): raise KernelError(f'{section} must be an object keyed by stable ID')
        for identity in graph[section]:
            if not isinstance(identity, str) or not identity.strip(): raise KernelError(f'Invalid {section} ID')
    names, bindings = set(), set()
    for identity, component in graph['components'].items():
        if not isinstance(component, dict) or set(component) - {'id', 'name', 'type', 'body_id', 'parameters', 'derivation', 'binding'}:
            raise KernelError(f'Invalid component {identity}')
        if component.get('id') != identity: raise KernelError(f'Component ID mismatch: {identity}')
        for key in ('name', 'type'):
            if not isinstance(component.get(key), str) or not component[key].strip():
                raise KernelError(f'Component {identity} requires {key}')
        if component['name'] in names: raise KernelError(f"Duplicate component name: {component['name']}")
        names.add(component['name'])
        binding = component.get('binding')
        if binding is not None:
            if not isinstance(binding, str) or not binding.strip(): raise KernelError('A component binding requires an imported native name')
            if binding in bindings: raise KernelError(f'Imported component {binding} is bound more than once')
            bindings.add(binding)
        body = component.get('body_id')
        if body is not None and (body not in doc.nodes or doc.nodes[body].kind not in ('body', 'instance')):
            raise KernelError(f'Component {identity} refers to missing CAD body {body}')
        parameters = component.get('parameters', {})
        if not isinstance(parameters, dict): raise KernelError(f'Component {identity} parameters must be an object')
        for key, value in parameters.items():
            if not isinstance(key, str) or not key or type(value) not in (int, float) or not math.isfinite(value):
                raise KernelError(f'Component {identity} parameter {key} must be a finite number')
        from .component_derivation import recipe_outputs
        recipe_outputs(component)
    occupied = set()
    for identity, connection in graph['connections'].items():
        if not isinstance(connection, dict) or set(connection) != {'id', 'ports'} or connection['id'] != identity:
            raise KernelError(f'Invalid connection {identity}')
        if not isinstance(connection['ports'], list) or not connection['ports']:
            raise KernelError(f'Connection {identity} requires ports')
        for port in connection['ports']:
            if not isinstance(port, dict) or set(port) != {'component_id', 'port'}:
                raise KernelError(f'Invalid endpoint in connection {identity}')
            if port['component_id'] not in graph['components']:
                raise KernelError(f"Connection {identity} refers to missing component {port['component_id']}")
            if not isinstance(port['port'], str) or not port['port']:
                raise KernelError(f'Connection {identity} requires a port name')
            endpoint = (port['component_id'], port['port'])
            if endpoint in occupied: raise KernelError(f'Port {endpoint} belongs to more than one connection')
            occupied.add(endpoint)
    return deepcopy(graph)


class ChangeGraph:
    label = 'Edit system components'

    def __init__(self, doc, graph):
        self.before = deepcopy(doc.component_graph)
        self.after = validate_graph(graph, doc)

    def do(self, doc):
        doc.component_graph = deepcopy(self.after)
        doc.touch()

    def undo(self, doc):
        doc.component_graph = deepcopy(self.before)
        doc.touch()

    def redo(self, doc):
        self.do(doc)


def compose_sources(sources, graph):
    """Lower captured graph values into the same registry-backed Rhai adapter.

    Keep editable sources intact; the worker retains this generated module in
    its resolved specification, with stable IDs linking declarations to the UI.
    """
    if not graph['components']: return deepcopy(sources), []
    module = str(PurePosixPath(sources['entry']).parent / '__robocad_graph.rhai')
    if module in sources['files']: raise KernelError(f'Reserved generated module already exists: {module}')
    quote = lambda value: json.dumps(value, ensure_ascii=False, allow_nan=False)
    lines, variables, mapping = [], {}, []
    for index, (identity, component) in enumerate(sorted(graph['components'].items())):
        variable = f'component_{index}'
        variables[identity] = variable
        name = 'graph/'+identity
        parameters = ', '.join(f'{quote(k)}: {quote(v)}' for k, v in sorted(component.get('parameters', {}).items()))
        binding = component.get('binding')
        call = f'bind_component({quote(name)}, {quote(binding)},' if binding else f'part({quote(name)},'
        lines.append(f'let {variable} = {call} {quote(component["type"])}, #{{{parameters}}});')
        mapping.append({'id': identity, 'name': component['name'], 'native_name': name,
                        'body_id': component.get('body_id'), 'source': module, 'line': len(lines)})
    for _, connection in sorted(graph['connections'].items()):
        ports = ', '.join(f'{variables[p["component_id"]]}.port({quote(p["port"])})' for p in connection['ports'])
        lines.append(f'connect([{ports}]);')
    result = deepcopy(sources)
    result['files'][module] = '\n'.join(lines)+'\n'
    result['files'][result['entry']] += '\nimport "__robocad_graph" as robocad_graph;\n'
    return result, mapping


class RegistryView:
    """Editor affordances from native declarations; compilation remains authoritative."""
    def __init__(self, catalogue, imported=()):
        self.types = {entry['type']: entry for entry in catalogue}
        self.imported = {key: entry for entry in imported for key in (entry['binding'], entry['name'])}
        self.connectors = {}
        for entry in catalogue:
            for port in entry['ports']:
                kind = port['schema'].get('Acausal')
                if isinstance(kind, str): self.connectors[kind] = port

    def descriptor(self, component):
        try: return self.types[component['type']]
        except KeyError: raise KernelError(f"Unknown native component type {component['type']}")

    @staticmethod
    def parameter_matches(pattern, name):
        if '*' not in pattern: return pattern == name
        a, b = pattern.split('*', 1)
        return name.startswith(a) and name.endswith(b) and len(name) > len(a)+len(b)

    def parameter(self, component, name):
        declarations = self.descriptor(component).get('parameters') or []
        return next((d for d in declarations if d['name'] == name), None) or next(
            (d for d in declarations if self.parameter_matches(d['name'], name)), None)

    def ports(self, component):
        native = self.imported.get(component.get('binding'))
        if native and native['type'] == component['type']: return native['ports']
        ports = []
        for declared in self.descriptor(component)['ports']:
            names = [declared['name']]
            if '*' in declared['name']:
                names = sorted(k for k in component.get('parameters', {}) if self.parameter_matches(declared['name'], k))
            for name in names:
                ports.append({**declared, 'name': name})
                kind = declared['schema'].get('Acausal')
                if isinstance(kind, dict) and 'Composite' in kind:
                    for member in kind['Composite']:
                        # Native member names use snake case; these are the
                        # member kinds currently registered in composite plugs.
                        native = {'FluidPh': 'fluid_ph', 'PlanarFrame': 'planar_frame'}.get(member, member.lower())
                        if member in self.connectors:
                            ports.append({**self.connectors[member], 'name': name+'.'+native})
        return ports

    def port(self, graph, endpoint):
        component = graph['components'][endpoint['component_id']]
        for port in self.ports(component):
            if port['name'] == endpoint['port']: return port
        if component.get('binding'):
            for declared in self.descriptor(component)['ports']:
                if '*' in declared['name'] and self.parameter_matches(declared['name'], endpoint['port']):
                    return {**declared, 'name': endpoint['port']}
        raise KernelError(f"{component['name']}.{endpoint['port']} is not a declared port")

    def validate_component(self, component):
        descriptor = self.descriptor(component)
        declarations = descriptor.get('parameters')
        if declarations is None: return
        values = component.get('parameters', {})
        from .component_derivation import recipe_outputs
        derived = recipe_outputs(component)
        for declaration in declarations:
            if declaration['required'] and not component.get('binding') and not any(self.parameter_matches(declaration['name'], k) for k in (*values, *derived)):
                raise KernelError(f"{component['name']}: {declaration['name']} is required [{declaration['unit']}]")
        for name, value in values.items():
            declaration = self.parameter(component, name)
            if declaration is None: raise KernelError(f"{component['name']}: unknown parameter {name}")
            lo, hi = declaration.get('minimum'), declaration.get('maximum')
            if (declaration.get('integer') and value != int(value)) or (lo is not None and
                (value < lo or (value == lo and declaration.get('exclusive_minimum')))) or (hi is not None and value > hi):
                raise KernelError(f"{component['name']}: {name} is outside its declared range [{declaration['unit']}]")

    def validate_connections(self, graph):
        for connection in graph['connections'].values():
            ports = [self.port(graph, p) for p in connection['ports']]
            kinds = [p['schema'].get('Acausal') for p in ports]
            if all(k is not None for k in kinds):
                composites = [k['Composite'] for k in kinds if isinstance(k, dict) and 'Composite' in k]
                if composites:
                    valid = all(c == composites[0] for c in composites) and all(
                        isinstance(k, dict) or k in composites[0] for k in kinds)
                else: valid = all(k == kinds[0] for k in kinds)
                if not valid: raise KernelError('Physical ports have incompatible connector types')
            elif any(k is not None for k in kinds):
                raise KernelError('A physical port cannot connect directly to a control signal')
            else:
                outputs = [p for p in ports if 'SignalOut' in p['schema']]
                if len(outputs) != 1: raise KernelError('A signal connection requires exactly one output')
                kinds = {next(iter(p['schema'].values())) for p in ports} - {'Dimensionless'}
                if len(kinds) > 1: raise KernelError('Signal units are incompatible')


def edit_graph(doc, ops, operation, expected_revision, catalogue):
    """Shared component/connection CRUD with atomic publication and revision checks."""
    from .candidates import check_revision
    with doc._lock:
        check_revision(doc, expected_revision)
        graph = deepcopy(doc.component_graph)
        registry = RegistryView(catalogue)
        action = operation['action']
        identity = operation.get('id') or uuid.uuid4().hex
        if action in ('add_component', 'update_component'):
            if action == 'add_component':
                if identity in graph['components']: raise KernelError(f'Component {identity} already exists')
                component = {'id': identity, **deepcopy(operation['component'])}
            else:
                component = {**graph['components'][identity], **deepcopy(operation['component'])}
            if component['id'] != identity: raise KernelError('Component identity cannot be changed')
            graph['components'][identity] = component
            validate_graph(graph, doc)
            registry.validate_component(component)
        elif action == 'delete_component':
            del graph['components'][identity]
            for cid, connection in list(graph['connections'].items()):
                connection['ports'] = [p for p in connection['ports'] if p['component_id'] != identity]
                if not connection['ports']:
                    del graph['connections'][cid]
                else:
                    # Preserve shared physical nodes and remaining signal
                    # receivers, unless their signal source was deleted.
                    ports = [registry.port(graph, p) for p in connection['ports']]
                    if all('SignalIn' in p['schema'] for p in ports): del graph['connections'][cid]
        elif action == 'connect':
            ports = deepcopy(operation['ports'])
            # Connecting to an existing node extends that node atomically.
            joined = []; joined_id = None
            for cid, connection in list(graph['connections'].items()):
                if any(p in connection['ports'] for p in ports):
                    joined_id = joined_id or cid
                    joined.extend(connection['ports']); del graph['connections'][cid]
            for port in joined:
                if port not in ports: ports.append(port)
            if not operation.get('id') and joined_id: identity = joined_id
            if identity in graph['connections']: raise KernelError(f'Connection {identity} already exists')
            graph['connections'][identity] = {'id': identity, 'ports': ports}
        elif action == 'delete_connection':
            del graph['connections'][identity]
        else: raise KernelError(f'Unknown system edit {action}')
        validate_graph(graph, doc)
        registry.validate_connections(graph)
        ops.set_component_graph(graph)
        return {'revision': doc.revision, 'id': identity, 'graph': deepcopy(graph)}
