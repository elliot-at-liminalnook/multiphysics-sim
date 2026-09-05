"""Explicit geometry-to-component recipes; dimensions cross from mm to SI here."""
from copy import deepcopy
from dataclasses import asdict
import math
from .kernel import KernelError, SurfaceKind
from .snapshots import digest


RECIPES = {
    'body_thermal_capacity': {'type': 'thermal.capacitance', 'outputs': {'heat_capacity': 'J/K'}},
    'circular_fluid_volume': {'type': 'fluid.pipe_ph', 'outputs': {'length': 'm', 'diameter': 'm', 'rise': 'm'}},
}


def recipe_outputs(component):
    recipe = component.get('derivation')
    if recipe is None: return {}
    if not isinstance(recipe, dict) or recipe.get('kind') not in RECIPES:
        raise KernelError('Unknown component derivation recipe')
    definition = RECIPES[recipe['kind']]
    if component['type'] != definition['type']:
        raise KernelError(f"{recipe['kind']} applies to {definition['type']}")
    if not component.get('body_id'): raise KernelError('A geometry derivation requires an attached CAD body')
    if recipe['kind'] == 'body_thermal_capacity':
        if set(recipe) - {'kind', 'specific_heat'}: raise KernelError('Unknown thermal-capacity recipe field')
        cp = recipe.get('specific_heat')
        if cp is not None and (type(cp) not in (int, float) or not math.isfinite(cp) or cp <= 0):
            raise KernelError('specific_heat must be positive and finite [J/(kg·K)]')
    else:
        if set(recipe) - {'kind', 'flow_direction'}: raise KernelError('Unknown circular-fluid-volume recipe field')
        direction = recipe.get('flow_direction', 1)
        if type(direction) not in (int, float) or direction not in (-1, 1):
            raise KernelError('flow_direction must be +1 or -1 along the CAD cylinder axis')
    overlap = set(definition['outputs']) & set(component.get('parameters', {}))
    if overlap: raise KernelError(f'Derived parameters also have explicit values: {sorted(overlap)}')
    return definition['outputs']


def derive_graph(graph, doc, cache=None):
    resolved = deepcopy(graph); evidence = []
    for identity, component in resolved['components'].items():
        outputs = recipe_outputs(component)
        if not outputs: continue
        node = doc.nodes[component['body_id']]
        body = doc.resolved_body(node.id)
        if body is None: raise KernelError(f"{component['name']}: attached body has no solid geometry")
        captured = doc._snapshot_body_cache.get(node.id)
        data = captured[1] if captured and captured[0] is body else doc.kernel.serialize(body)
        geometry_hash = digest(data)
        get = lambda stage, dependencies, build: cache.get(stage, dependencies, build) if cache else build()
        props = get('body_properties', {'geometry': geometry_hash}, lambda: asdict(doc.kernel.mass_properties(body)))
        if not math.isfinite(props['volume']) or props['volume'] <= 0:
            raise KernelError(f"{component['name']}: derivation requires a positive finite solid volume")
        recipe = component['derivation']
        inputs = {'body_id': node.id, 'geometry_hash': geometry_hash, 'volume_mm3': props['volume']}
        if recipe['kind'] == 'body_thermal_capacity':
            material = doc.materials.get(node.material)
            if material is None: raise KernelError(f"{component['name']}: assign a material to the body")
            density = material.density*1000.  # g/cm³ → kg/m³
            cp = recipe.get('specific_heat', material.props()['specific_heat'])
            if not math.isfinite(density) or density <= 0 or not math.isfinite(cp) or cp <= 0:
                raise KernelError('Thermal derivation requires positive density and specific heat')
            mass = props['volume']*1e-9*density
            values = {'heat_capacity': mass*cp}
            inputs.update(material_id=material.id, density_kg_m3=density, specific_heat_j_kg_k=cp,
                          specific_heat_source='recipe' if 'specific_heat' in recipe else 'material', mass_kg=mass)
            formula = 'heat_capacity = volume × density × specific_heat'
            limitations = ['Uniform body temperature; constant material properties; full CAD solid volume.']
        else:
            faces = get('body_faces', {'geometry': geometry_hash}, lambda: [f.to_json() for f in doc.kernel.faces(body)])
            cylinders = [f for f in faces if f['kind'] == SurfaceKind.CYLINDER.value]
            caps = [f for f in faces if f['kind'] == SurfaceKind.PLANE.value]
            if len(faces) != 3 or len(cylinders) != 1 or len(caps) != 2:
                raise KernelError(f"{component['name']}: select a closed circular cylinder representing the fluid volume, not the duct wall")
            cylinder = cylinders[0]; radius = cylinder['radius']; axis = cylinder['axis_dir']
            dot = lambda a, b: sum(x*y for x, y in zip(a, b))
            norm = math.sqrt(dot(axis, axis)); axis = [v/norm for v in axis]
            caps.sort(key=lambda f: dot(f['centroid'], axis))
            vector = [b-a for a, b in zip(caps[0]['centroid'], caps[1]['centroid'])]
            length = dot(vector, axis)
            area = math.pi*radius**2
            aligned = all(abs(abs(dot(f['normal'], axis))-1) < 1e-7 for f in caps)
            aligned &= sum((v-length*a)**2 for v, a in zip(vector, axis)) < 1e-10*max(1., length**2)
            if length <= 0 or radius <= 0 or not aligned or not math.isclose(props['volume'], area*length, rel_tol=1e-6):
                raise KernelError(f"{component['name']}: fluid-volume cylinder must have full circular caps normal to its axis")
            if any(not math.isclose(f['area'], area, rel_tol=1e-6) for f in caps):
                raise KernelError('Fluid-volume end caps must be full circles')
            direction = recipe.get('flow_direction', 1)
            a, b = [f['centroid'] for f in caps][::int(direction)]
            values = {'length': length*.001, 'diameter': 2*radius*.001, 'rise': (b[2]-a[2])*.001}
            inputs.update(representation='fluid_volume', axis=axis, end_a_mm=a, end_b_mm=b, area_m2=area*1e-6)
            formula = 'length = cap separation; diameter = 2 × radius; rise = z(b) − z(a)'
            limitations = ['Straight uniform circular passage; native lumped water pipe model; no bends or local losses inferred.']
        component.setdefault('parameters', {}).update(values)
        evidence.append({'component_id': identity, 'name': component['name'], 'recipe': deepcopy(recipe),
            'inputs': inputs, 'outputs': {k: {'value': v, 'unit': outputs[k]} for k, v in values.items()},
            'formula': formula, 'limitations': limitations})
        component.pop('derivation')
    return resolved, evidence
