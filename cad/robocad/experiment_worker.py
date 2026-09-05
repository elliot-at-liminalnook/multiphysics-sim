"""Worker process: derive a captured CAD model, then run captured Rhai sources."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from .snapshots import canonical,digest
from .experiments import write_json, package_versions, binary_digest, source_files
from .experiment_results import evaluate_expectations
from .experiment_config import settings, object_fields, cad_overrides, source_context, PROFILES
from .kernel import KernelError
from . import experiment_records


_folder = None


def emit(**event):
    if _folder:
        fields = {k: v for k, v in event.items() if k in ('state', 'stage', 'fraction', 'error', 'cache', 'timing', 'evaluation', 'exit_code')}
        experiment_records.update(_folder, **fields)
    try: print(json.dumps(event),flush=True)
    except BrokenPipeError:
        # The editor can disappear while the worker still owns its run lease.
        # Keep finalizing records and artifacts for the next editor to observe.
        sys.stdout = open(os.devnull, 'w')


def derivation_identity():
    files={name:digest(data) for name,data in source_files().items()}
    packages=package_versions()
    return {'sources':files,'packages':packages}


def main(folder,binary):
    global _folder
    folder=Path(folder)
    _folder = folder
    if not Path(binary).is_file():raise ValueError('Build the experiment runner: cargo build --release --bin sim-experiment')
    started=time.perf_counter()
    spec=json.loads((folder/'input.json').read_text())
    source_identity=derivation_identity()
    if spec['provenance'].get('derivation') != source_identity:
        raise ValueError('Captured derivation sources or installed dependency versions changed')
    if spec['provenance'].get('python_version') != sys.version:
        raise ValueError('Python interpreter version changed after capture')
    if spec['provenance'].get('binary_hash') != binary_digest(Path(binary).read_bytes()):
        raise ValueError('Captured simulator binary failed its content check')
    from .component_graph import compose_sources, empty_graph
    graph = spec.pop('component_graph', empty_graph())
    graph_derivations = []
    graph_derivation_started = time.perf_counter()
    recipe_components = {identity: component for identity, component in graph['components'].items() if component.get('derivation')}
    graph_cache = None
    if recipe_components:
        emit(state='building', stage='component derivation')
        from .derivation_cache import DerivationCache
        from .component_derivation import derive_graph
        graph_cache = DerivationCache(folder.parent/'derived', source_identity)
        def build_recipes():
            from .document import Document
            doc = Document.load(str(folder/'model.rcad'))
            return derive_graph({'version': 1, 'components': recipe_components, 'connections': {}}, doc, graph_cache)
        resolved_recipes, graph_derivations = graph_cache.get('component_recipes',
            {'cad': spec['provenance'].get('cad_derivation_hash', spec['provenance'].get('physical_hash')),
             'components': recipe_components}, build_recipes)
        graph['components'].update(resolved_recipes['components'])
    write_json(folder/'component_derivations.json', graph_derivations)
    graph_derivation_seconds = time.perf_counter()-graph_derivation_started
    spec['system'], graph_mapping = compose_sources(spec['system'], graph)
    write_json(folder/'component_graph_mapping.json', graph_mapping)
    write_json(folder/'composition.json', spec)
    emit(state='building',stage='resolve')
    resolved=subprocess.run([binary,'resolve',str(folder/'composition.json')],capture_output=True,text=True,check=False)
    if resolved.returncode:raise ValueError(resolved.stderr.strip() or resolved.stdout.strip())
    plan=json.loads(resolved.stdout)
    spec['provenance']['uses_cad'] = bool(plan['cad']) or bool(recipe_components)
    write_json(folder/'system.json',plan)
    script_mapping = [{'native_name': c['name'], 'name': c['name'], 'source': c['location']['source'],
        'line': c['location'].get('line'), 'column': c['location'].get('column')} for c in plan['components']]
    write_json(folder/'script_component_mapping.json', script_mapping)
    if (spec.get('controller') or {}).get('language') == 'process':
        from .experiment_process import prepare
        prepare(spec['controller'], folder, spec['seed'])
    configuration=plan.get('configuration') or {}
    with source_context(plan.get('configuration_location')):
        object_fields(configuration, ('settings', 'expectations', 'cad_overrides'), 'configure')
        object_fields(configuration.get('settings', {}), PROFILES[spec['profile']], 'configure.settings')
    configuration_evidence = {'profile': spec['profile'], 'profile_defaults': PROFILES[spec['profile']],
        'captured_settings': spec['settings'], 'script_settings': configuration.get('settings', {}),
        'source': plan.get('configuration_location'), 'seed': spec['seed'], 'cad_overrides': []}
    excluded_bodies = sorted({c['body_id'] for c in recipe_components.values() if c['derivation']['kind'] == 'circular_fluid_volume'})
    configuration_evidence['mechanical_exclusions'] = [{'body_id': body_id, 'reason': 'Explicit fluid-volume representation'} for body_id in excluded_bodies]
    with source_context(plan.get('configuration_location')):
        spec['settings'] = settings({**spec['settings'], **configuration.get('settings', {})}, spec['profile'])
    configuration_evidence['effective_settings'] = spec['settings']
    export_started=time.perf_counter()
    cache_hit=False
    derived_cache = None
    if plan['cad']:
        if not (folder/'model.rcad').exists():raise ValueError('System imports CAD, but this run has no captured document')
        if len(plan['cad'])!=1:raise ValueError('Use one captured assembly per experiment')
        emit(state='building',stage='cad')
        cad_hash = spec['provenance'].get('cad_derivation_hash', spec['provenance']['physical_hash'])
        key=digest(canonical({'cad_derivation_hash':cad_hash,'mechanical_exclusions':excluded_bodies,'flex':spec['settings']['flex'],'derivation':source_identity}))
        cache=folder.parent/'cache'/key
        model=None
        try:
            cached=(cache/'model.json').read_bytes()
            metadata=json.loads((cache/'metadata.json').read_text())
            if metadata['content_hash']==digest(cached):
                model=json.loads(cached);cache_hit=True
        except (OSError,ValueError,KeyError):pass
        if model is None:
            # Cached controller iterations do not need to import the CAD stack.
            from .document import Document
            from .physical import export_physical_model
            from .derivation_cache import DerivationCache
            doc=Document.load(str(folder/'model.rcad'))
            if excluded_bodies:
                def references_proxy(value):
                    if isinstance(value, str): return value in excluded_bodies
                    if isinstance(value, dict): return any(references_proxy(v) for v in value.values())
                    if isinstance(value, list): return any(references_proxy(v) for v in value)
                    return False
                for node in doc.walk():
                    if node.id in excluded_bodies: continue
                    if references_proxy(node.joint.to_json() if node.joint else {}) or references_proxy(node.robot or {}) or node.source in excluded_bodies:
                        raise KernelError(f'{node.name} mechanically references a fluid-volume proxy. Attach mechanical joints, sensors and mounts to the duct wall body instead.')
                for body_id in excluded_bodies: doc.remove(body_id)
            derived_cache = DerivationCache(folder.parent/'derived', source_identity)
            model=export_physical_model(doc,flex=spec['settings']['flex'],cache=derived_cache)
            # Source paths/timestamps belong to the run, not the reusable model.
            model['source']={'cad_derivation_hash':cad_hash}
            cache.mkdir(parents=True,exist_ok=True)
            write_json(cache/'model.json',model)
            write_json(cache/'metadata.json',{'content_hash':digest((cache/'model.json').read_bytes()),'key':key})
        spec['cad'][plan['cad'][0]]=model
        with source_context(plan.get('configuration_location')):
            configuration_evidence['cad_overrides'] = cad_overrides(model, configuration.get('cad_overrides', []), plan.get('configuration_location'))
        # Keep geometry-cache entries independent of scenario randomness.
        # Seed zero preserves the CAD seed; every explicit run seed selects a
        # deterministic, independent stream in the native sensor model.
        configuration_evidence['cad_seed'] = model.get('uncertainty', {}).get('seed', 0)
        model.setdefault('uncertainty', {})['seed'] = int(configuration_evidence['cad_seed']) ^ spec['seed']
        configuration_evidence['effective_cad_seed'] = model['uncertainty']['seed']
        if not spec['settings']['noise']:
            for sensor in model.get('sensors', []):
                sensor['noise'] = {k: 0. for k in sensor.get('noise', {})}
                sensor['bias_walk'] = 0.
        write_json(folder/'physical.json',model)
        if spec['settings']['flex']:
            failures = [f'{link["name"]} ({link.get("id", "unknown ID")}): {link["flex_error"]}'
                        for link in model['links'] if link.get('flex_error')]
            if failures:
                raise KernelError('Flex derivation failed; the experiment will not substitute a rigid model. '
                    'Review the captured physical.json, correct the attachment patches/geometry, '
                    'or explicitly disable flex.\n'+'\n'.join(failures))
    elif configuration.get('cad_overrides'):
        with source_context(plan.get('configuration_location')):
            raise KernelError('cad_overrides requires an imported CAD assembly')
    write_json(folder/'configuration.json', configuration_evidence)
    spec['provenance']['resolved_model_hash']=digest(canonical(spec['cad']))
    write_json(folder/'specification.json',spec)
    export_seconds=time.perf_counter()-export_started
    emit(state='building',stage='compile',cache={'cad_hit':cache_hit},timing={'derive_s':export_seconds})
    process=subprocess.Popen([binary,'run',str(folder/'specification.json'),str(folder)],stdout=subprocess.PIPE,text=True)
    diagnostic=None
    for line in process.stdout:
        try:
            event=json.loads(line)
            if event.get('error'):diagnostic=event['error']
            # Rust completion precedes Python mapping/evaluation. Finalize only
            # after every artifact and measured check has been written.
            if event.get('state') not in ('building', 'running'): event.pop('state', None)
            emit(**event)
        except ValueError:pass
    code=process.wait()
    if code:raise ValueError(diagnostic or f'Simulator failed with exit code {code}')
    result=json.loads((folder/'result.json').read_text())
    result['timing']['derive_s']=export_seconds
    result['timing']['component_derive_s'] = graph_derivation_seconds
    result['timing']['worker_total_s']=time.perf_counter()-started
    result['cache']={'cad_hit':cache_hit, 'derived': derived_cache.stats if derived_cache else {}}
    result['cache']['component_derivations'] = graph_cache.stats if graph_cache else {}
    result['configuration'] = configuration_evidence
    result['component_graph_mapping'] = graph_mapping
    result['component_derivations'] = graph_derivations
    result['script_component_mapping'] = script_mapping
    result['limitations'] = ['Accuracy bounds apply only to the documented geometry, material and boundary conditions.',
        'Flex replay shows attachment displacement arrows with rigid CAD meshes, not full surface deformation.',
        'A seeded run does not sweep the captured material/geometry Monte Carlo uncertainty distributions.',
        'CAD sensor noise flag controls stochastic noise and bias walk; fixed bias and quantization remain captured CAD inputs.',
        'Scripted stochastic components use their declared parameters; use seed() to select reproducible streams.']
    result['expectations']=configuration.get('expectations',[])
    # Carry IDs through display-name keyed legacy simulator summaries. Replay
    # uses link members; channel filtering also needs joint and motor IDs.
    result['cad_mapping'] = [dict(item, section='links') for item in result.get('cad_mapping', [])]
    for model in spec['cad'].values():
        link_ids = {link['name']: link.get('members', [link['id']]) for link in model['links']}
        for section in ('joints', 'motors'):
            for item in model[section]:
                result['cad_mapping'].append({'section': section, 'name': item['name'], 'id': item.get('id'),
                    'related_ids': list(dict.fromkeys(nid for key in ('parent', 'child', 'mounted_on')
                        for nid in link_ids.get(item.get(key), [])))})
    result['evaluation'] = {'status': 'not_simulated', 'metrics': []} if spec.get('preflight') else evaluate_expectations(result, result['expectations'])
    objectives = {}
    if spec['cad']:
        links = [link for model in spec['cad'].values() for link in model['links']]
        objectives['mass/total'] = {'value': sum(link['mass'] for link in links), 'unit': 'kg'}
        objectives['mass/moving'] = {'value': sum(link['mass'] for link in links if not link.get('ground')), 'unit': 'kg'}
    for metric in result['evaluation']['metrics']:
        objectives['expectation/'+metric['name']] = {'value': metric['value'], 'unit': metric['unit'],
            'definition': {k: v for k, v in metric.items() if k not in ('value', 'passed', 'samples', 'min', 'max')}}
    result['objectives'] = objectives
    write_json(folder/'result.json',result)
    emit(state='completed',stage='checked' if spec.get('preflight') else 'finished',fraction=1,exit_code=0,
         timing=result['timing'],cache=result['cache'],evaluation=result['evaluation'])


if __name__=='__main__':
    try:main(*sys.argv[1:])
    except Exception as error:
        emit(state='failed',error=f'{type(error).__name__}: {error}')
        print(str(error),file=sys.stderr)
        sys.exit(1)
