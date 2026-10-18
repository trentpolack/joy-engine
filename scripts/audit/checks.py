# Copyright (c) 2026 Trent Polack. All Rights Reserved.
# Licensed under the MIT License.

"""Repository snapshots and read-only maintenance checks shared with consumers."""

import ast
from collections import Counter, defaultdict
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import shlex
import subprocess
from urllib.parse import unquote, urljoin, urlsplit
from urllib.request import url2pathname


GENERATED = {'dist', 'dist-rust', 'target', 'node_modules', 'artifacts', 'coverage',
             '.vite', '.cache', 'playwright-report', 'test-results'}
FORMATS = {'.joylevel': 'joylevel', '.joyobject': 'joyobject', '.joyproject': 'joyproject',
           '.joyfx': 'joyfx', '.formlab': 'formlab', '.joyconfig': 'joyconfig'}
MAX_TEXT_BYTES = 16 * 1024 * 1024
INVALID_JSON = object()
TEXT_SUFFIXES = {'.rs', '.wgsl', '.mjs', '.js', '.cjs', '.ts', '.tsx', '.py', '.json',
                 '.jsonc', '.toml', '.yaml', '.yml', '.md', '.html', '.css', '.txt',
                 '.form', *FORMATS}
MANUAL_REVIEW = [
    'Test value: small or similar test bodies are review signals, not deletion instructions. '
    'Check which behavior, ownership boundary, regression, or compatibility contract each protects.',
    'Schema/runtime parity: validating examples does not prove that JSON schemas match all Rust '
    'defaults, allowed fields, limits, or error paths. Compare the parsers and schema tables manually.',
    'Unused assets/code: no literal filename reference does not prove an item is unused. '
    'Check dynamic discovery, packaging, and intentionally retained source art.',
    'Documentation meaning: validate current instructions and private-host prerequisites. '
    'Historical changelog references and dated plans can remain useful.',
    'Configuration relevance: inspect CI, Cargo/npm workspaces, editor settings, cleanup coverage, '
    'and deployment behavior. Existing files/targets do not establish that settings remain appropriate.',
    'Retain active Node/Vite tooling, wasm-bindgen glue, serialized compatibility behavior, '
    'recorded parity, and third-party attribution unless their consumers are deliberately removed.',
]


def git(root, *args, required=True):
    result = subprocess.run(['git', '--no-optional-locks', '-c', 'core.fsmonitor=false',
                             '-C', str(root), *args], capture_output=True, timeout=30)
    if result.returncode and required:
        raise ValueError(f'Git failed in {root}: {result.stderr.decode(errors="replace").strip()}')
    if result.returncode:
        return None
    decoded = result.stdout.decode('utf-8', errors='replace')
    return decoded if '-z' in args else decoded.strip()


class Repository:
    """Tracked working-tree snapshot. Never follows file symlinks outside the checkout."""

    def __init__(self, name, root):
        self.name = name
        self.root = root.resolve()
        top = git(self.root, 'rev-parse', '--show-toplevel')
        if Path(top).resolve() != self.root:
            raise ValueError(f'{self.root} is not a repository root (possibly an uninitialized submodule).')
        self.head = git(self.root, 'rev-parse', '--verify', 'HEAD', required=False)
        self.status = git(self.root, 'status', '--porcelain', '--untracked-files=no')
        self.files = sorted(filter(None, git(self.root, 'ls-files', '-z').split('\0')))
        self.text = {}
        self.sizes = {}
        self.skipped = []
        for name in self.files:
            path = self.root / name
            try:
                outside = not path.resolve().is_relative_to(self.root)
                linked = any(p.is_symlink() for p in (path, *path.parents) if p != self.root and p.is_relative_to(self.root))
            except (OSError, RuntimeError):
                outside, linked = True, True
            if outside or linked or not path.is_file():
                self.skipped.append({'path': name, 'reason': 'symlink, submodule, or missing working-tree file'})
                continue
            size = path.stat().st_size
            self.sizes[name] = size
            if is_generated(name) or path.name in {'package-lock.json', 'Cargo.lock'}:
                continue
            if size > MAX_TEXT_BYTES:
                self.skipped.append({'path': name, 'reason': 'exceeds text scan size limit'})
                continue
            if path.suffix not in TEXT_SUFFIXES and path.suffix and path.name not in {'LICENSE', 'AGENTS.md'}:
                continue
            try:
                self.text[name] = path.read_text(encoding='utf-8')
            except (UnicodeError, OSError):
                self.skipped.append({'path': name, 'reason': 'not readable UTF-8 text'})


def is_generated(path):
    return bool(set(Path(path).parts) & GENERATED)


class Audit:
    """Scan one owner; optional schema/path context supplies references, never extra findings.

    Consumers can reuse checks without rescanning the engine or embedding host
    policy here. Paths are absolute; mappings redirect a hosted path to a checkout.
    """

    def __init__(self, repository, *, schema_repository=None, path_mappings=(),
                 private_repositories=(), architecture=False):
        self.repository = repository
        self.repositories = (repository,)
        self.schema_repository = schema_repository or repository
        self.context = (repository,) if self.schema_repository is repository else (repository, self.schema_repository)
        self.path_mappings = tuple((source.resolve(), target.resolve()) for source, target in path_mappings)
        self.private_repositories = [name.lower().removesuffix('.git') for name in private_repositories]
        self.architecture = architecture
        self.findings = []
        self.tests = []
        self.schema_status = {'status': 'not run'}
        self.stats = Counter()

    def add(self, rule, repo, path, message, *, kind='issue', line=None, **extra):
        if repo is not self.repository:
            return
        item = {'rule': rule, 'kind': kind, 'repository': repo.name,
                'path': path, 'message': message, **extra}
        if line:
            item['line'] = line
        self.findings.append(item)

    def resolve_path(self, path):
        """Resolve local references through explicit consumer-owned path mappings."""
        path = path.resolve()
        for source, target in self.path_mappings:
            if path.is_relative_to(source):
                return target / path.relative_to(source)
        return path

    def available(self, path):
        path = self.resolve_path(path)
        return any(path.is_relative_to(r.root) for r in self.context) and path.exists()

    def parse_json(self, repo, path):
        try:
            return json.loads(repo.text[path])
        except (ValueError, KeyError) as error:
            self.add('invalid_json', repo, path, str(error))
            return INVALID_JSON

    def package_map(self, repo, path, package, key):
        value = package.get(key)
        if value is None:
            return {}
        if not isinstance(value, dict):
            self.add('invalid_package_metadata', repo, path, f'Package field {key!r} must be an object to inspect it.')
            return {}
        return value

    def scan(self, skip_schemas=False):
        self.scan_artifacts()
        self.scan_package_targets()
        self.scan_docs()
        if self.architecture:
            self.scan_architecture()
        self.scan_tests()
        self.scan_schemas(skip_schemas)
        return self.report()

    def scan_artifacts(self):
        corpus = [(repo, path, text) for repo in self.repositories for path, text in repo.text.items()]
        for repo in self.repositories:
            groups = defaultdict(list)
            for path in repo.files:
                parts = Path(path).parts
                for i, part in enumerate(parts):
                    if part in GENERATED:
                        groups['/'.join(parts[:i + 1])].append(path)
                        break
            for folder, files in sorted(groups.items()):
                size = sum(repo.sizes.get(p, 0) for p in files)
                self.add('tracked_generated_output', repo, folder,
                         f'{len(files)} tracked files, {size:,} bytes in a conventional output directory. '
                         'Check publication/archive requirements before removing.',
                         kind='review', files=files, bytes=size)
            for path in repo.files:
                if is_generated(path) or Path(path).suffix not in {'.wgsl', '.png', '.jpg', '.wav'}:
                    continue
                basename = Path(path).name
                hits = any(basename in text for owner, source, text in corpus
                           if (owner.name, source) != (repo.name, path))
                if not hits:
                    self.add('unreferenced_asset_name', repo, path,
                             'No literal filename reference found in scanned authored text in this repository. '
                             'Dynamic loading and source/reference art require manual review.', kind='review')

    def scan_package_targets(self):
        for repo in self.repositories:
            for path in repo.text:
                if Path(path).name != 'package.json':
                    continue
                package = self.parse_json(repo, path)
                if not isinstance(package, dict):
                    continue
                self.stats['package_manifests'] += 1
                base = (repo.root / path).parent
                for name, command in self.package_map(repo, path, package, 'scripts').items():
                    if not isinstance(command, str):
                        continue
                    try:
                        tokens = shlex.split(command)
                    except ValueError:
                        continue
                    if any(token in {'cd', 'pushd', 'popd'} for token in tokens):
                        self.add('script_target_review', repo, path,
                                 f'Script {name!r} changes shell working directory; static target checks skipped.',
                                 kind='review', script=name)
                        continue
                    for index, token in enumerate(tokens[:-1]):
                        # Flags, dynamic shell expressions, -e, and test globs are deliberately not interpreted.
                        if token not in {'node', 'python', 'python3'}:
                            continue
                        target = tokens[index + 1]
                        if target.startswith('-') or any(c in target for c in '*?$`'):
                            continue
                        if not self.available(base / target):
                            self.add('missing_script_target', repo, path,
                                     f'Package script {name!r} references missing target {target!r}.',
                                     line=line_of(repo.text[path], f'"{name}"'), script=name, target=target)
                targets = dict(package.get('bin', {})) if isinstance(package.get('bin'), dict) else {}
                if isinstance(package.get('bin'), str):
                    targets['bin'] = package['bin']
                for label, target in walk_string_values(package.get('exports', {})):
                    if target.startswith('./'):
                        targets[f'exports {label}'] = target
                for label, target in targets.items():
                    if not isinstance(target, str):
                        self.add('invalid_package_metadata', repo, path, f'{label} is not a string target.')
                    elif '*' not in target and not self.available(base / target):
                        self.add('missing_package_target', repo, path,
                                 f'{label} points to missing file {target!r}.', target=target)
                dependencies = {**self.package_map(repo, path, package, 'dependencies'),
                                **self.package_map(repo, path, package, 'devDependencies')}
                for name in dependencies:
                    if name.startswith('@luma.gl/') or name in {'three', 'cannon-es', 'pixi.js'}:
                        self.add('legacy_runtime_dependency', repo, path,
                                 f'{name} remains declared. Verify its current consumer before removing.', kind='review')

    def scan_docs(self):
        for repo in self.repositories:
            for path, text in repo.text.items():
                if not path.endswith('.md'):
                    continue
                self.stats['markdown_files'] += 1
                # Code samples are historical/code data, not rendered Markdown links.
                visible = re.sub(r'(?ms)^\s*(`{3,}|~{3,}).*?^\s*\1[^\n]*$',
                                 lambda m: '\n' * m.group().count('\n'), text)
                for match in re.finditer(r'https://github\.com/([^/\s)]+/[^/\s)#]+)([^\s)>]*)', visible):
                    if match[1].lower().removesuffix('.git') in self.private_repositories:
                        self.add('private_documentation_link', repo, path,
                                 'Public engine documentation links to the configured private repository. '
                                 'Check whether this is required guidance or optional historical context.',
                                 kind='review', line=line_at(visible, match.start()), target=match.group())
                links = list(re.finditer(r'!?\[[^\]\n]*\]\(\s*(<[^>]+>|[^\s)]+)(?:\s+["\'][^\n]*?["\'])?\s*\)', visible))
                links += list(re.finditer(r'(?m)^\s*\[[^\]]+\]:\s*(<[^>]+>|\S+)', visible))
                for match in links:
                    target = match[1].strip('<>')
                    parsed = urlsplit(target)
                    if parsed.scheme or parsed.netloc:
                        continue
                    self.stats['local_doc_links'] += 1
                    candidate = self.resolve_path((repo.root / path).parent / unquote(parsed.path)) if parsed.path else repo.root / path
                    if not self.available(candidate):
                        self.add('broken_doc_link', repo, path, f'Local Markdown destination is missing: {target}',
                                 line=line_at(visible, match.start()), target=target)
                    elif parsed.fragment and candidate.suffix == '.md':
                        try:
                            destination = candidate.read_text(encoding='utf-8')
                        except (OSError, UnicodeError):
                            continue
                        if unquote(parsed.fragment) not in markdown_anchors(destination):
                            self.add('doc_anchor_review', repo, path,
                                     f'Fragment was not found by the heading/HTML-id scanner: {target}',
                                     kind='review', line=line_at(visible, match.start()), target=target)

    def scan_architecture(self):
        repo = self.repository
        path = 'architecture/layers.json'
        if path not in repo.text:
            self.add('architecture_policy_absent', repo, path, 'No layer policy found; relevance needs manual review.', kind='review')
            return
        policy = self.parse_json(repo, path)
        if not isinstance(policy, dict):
            return
        layers = policy.get('layers', {})
        if not isinstance(layers, dict):
            self.add('invalid_layer_policy', repo, path, 'layers must be an object.')
            return
        covered = set()
        covered_crates = set(policy.get('testSupportCrates', []))
        for layer, definition in layers.items():
            if (not isinstance(definition, dict) or any(
                    not isinstance(definition.get(key, []), list) or
                    not all(isinstance(v, str) for v in definition.get(key, []))
                    for key in ('roots', 'crates', 'mayImport'))):
                self.add('invalid_layer_policy', repo, path, f'Layer {layer!r} requires string arrays roots/crates/mayImport.')
                continue
            for root in definition.get('roots', []):
                normalized = root.replace('-', '_')
                covered.add(normalized)
                if not any((repo.root / 'src' / name).exists() for name in (root, normalized, normalized + '.rs')):
                    self.add('stale_layer_root', repo, path,
                             f'Layer {layer} declares {root!r}, but that source module does not exist.', line=line_of(repo.text[path], f'"{root}"'))
            for crate in definition.get('crates', []):
                covered_crates.add(crate)
                if not (repo.root / 'crates' / crate / 'Cargo.toml').exists():
                    self.add('stale_layer_crate', repo, path,
                             f'Layer {layer} declares crate {crate!r}, but crates/{crate}/Cargo.toml does not exist.', line=line_of(repo.text[path], f'"{crate}"'))
            for target in definition.get('mayImport', []):
                if target not in layers:
                    self.add('unknown_layer_dependency', repo, path, f'{layer} mayImport references unknown layer {target!r}.')
        modules = set()
        for source in repo.files:
            parts = Path(source).parts
            if source.endswith('.rs') and len(parts) > 1 and parts[0] == 'src':
                modules.add(Path(parts[1]).stem)
        for module in sorted(modules - covered - {'lib', 'bin', 'main'}):
            self.add('unmapped_source_module', repo, path, f'Source module {module!r} has no layer assignment.')
        crates = {Path(source).parts[1] for source in repo.files
                  if len(Path(source).parts) == 3 and Path(source).parts[0] == 'crates' and Path(source).parts[2] == 'Cargo.toml'}
        for crate in sorted(crates - covered_crates):
            self.add('unmapped_engine_crate', repo, path, f'Engine crate {crate!r} has no layer assignment.')
        self.stats['architecture_references'] = sum('architecture/layers.json' in text for p, text in repo.text.items() if p != path)

    def scan_tests(self):
        duplicate_bodies = defaultdict(list)
        protected = re.compile(r'save|serial|round.?trip|resource|leak|cleanup|dispose|drop|budget|bound|limit|'
                               r'overflow|invalid|reject|rollback|undo|determin|parity|fallback|shader|uniform|'
                               r'input|state|transition|finite|cancel|timeout|deadline|failed|error|corrupt', re.I)
        for repo in self.repositories:
            for path, text in repo.text.items():
                if path.endswith('.rs'):
                    masked = mask_rust(text)
                    pattern = r'#\[test\]\s*(?:#\[[^\]]*\]\s*)*(?:async\s+)?fn\s+(\w+)\s*\('
                    for match in re.finditer(pattern, masked):
                        start = masked.find('{', match.end())
                        if start < 0:
                            continue
                        depth, end = 1, start + 1
                        while end < len(masked) and depth:
                            depth += (masked[end] == '{') - (masked[end] == '}')
                            end += 1
                        body = text[start + 1:end - 1]
                        code = masked[start + 1:end - 1]
                        record = {'repository': repo.name, 'path': path, 'line': line_at(text, match.start()),
                                  'name': match[1], 'language': 'Rust'}
                        self.tests.append(record)
                        assertions = len(re.findall(r'\bassert(?:_eq|_ne)?!\s*\(', code))
                        statements = code.count(';')
                        if 1 <= assertions <= 2 and statements <= 3 and not protected.search(match[1]) and not re.search(r'\b(for|while|loop|match|if)\b', code):
                            self.add('small_test_review', repo, path,
                                     'Small direct-assertion test. Review whether it protects a distinct contract or '
                                     'only repeats a helper/default/presentation value. Size alone does not justify deletion.',
                                     kind='review', line=record['line'], test=match[1])
                        signature = re.sub(r'\s+', ' ', body).strip()
                        if signature:
                            duplicate_bodies[signature].append(record)
                elif path.endswith(('.mjs', '.js', '.cjs', '.ts')) and ('test' in path or 'spec' in path):
                    for match in re.finditer(r'(?m)^\s*(?:test|it)(?:\.(?:skip|only))?\(\s*([\'"`])(.+?)\1', text):
                        self.tests.append({'repository': repo.name, 'path': path, 'line': line_at(text, match.start()),
                                           'name': match[2], 'language': 'Node declaration'})
                elif path.endswith('.py') and 'test' in path:
                    try:
                        nodes = ast.walk(ast.parse(text))
                        for node in nodes:
                            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name.startswith('test_'):
                                self.tests.append({'repository': repo.name, 'path': path, 'line': node.lineno,
                                                   'name': node.name, 'language': 'Python'})
                    except SyntaxError:
                        pass
        by_name = {repo.name: repo for repo in self.repositories}
        for records in duplicate_bodies.values():
            if len(records) > 1:
                first = records[0]
                self.add('similar_test_bodies', by_name[first['repository']], first['path'],
                         'Multiple Rust tests have the same body after whitespace normalization. '
                         'Different imports/implementations may make every copy necessary.',
                         kind='review', line=first['line'], occurrences=records)

    def scan_schemas(self, skip):
        self.schema_status = {'status': 'skipped', 'schemas_checked': 0, 'references_checked': 0,
                              'schemas_failed': 0, 'documents_checked': 0, 'documents_failed': 0,
                              'documents_unvalidated': 0}
        if skip:
            self.schema_status['reason'] = 'Requested with --skip-schema-validation.'
            return
        try:
            from jsonschema.validators import validator_for
            from referencing import Registry, Resource
            from referencing.jsonschema import DRAFT7
        except ImportError:
            self.schema_status['reason'] = "Install jsonschema>=4.18 to enable validation; no schema checks passed or failed."
            return
        schemas = {}
        for repo in self.context:
            for path in repo.text:
                if path.endswith('.schema.json'):
                    schema = self.parse_json(repo, path)
                    if schema is INVALID_JSON:
                        self.schema_status['schemas_failed'] += int(repo is self.repository)
                        continue
                    try:
                        if not isinstance(schema, (dict, bool)):
                            raise ValueError('A JSON Schema must be an object or boolean.')
                        if isinstance(schema, dict) and '$schema' in schema and not isinstance(schema['$schema'], str):
                            raise ValueError('$schema must be a string URI.')
                        cls = validator_for(schema, default=None)
                        if cls is None and isinstance(schema, dict) and '$schema' in schema:
                            self.add('schema_dialect_unavailable', repo, path,
                                     'Declared schema dialect is not supported by the installed validator.', kind='review')
                            continue
                        (cls or validator_for(schema)).check_schema(schema)
                        Resource.from_contents(schema, default_specification=DRAFT7)
                        schemas[(repo.root / path).as_uri()] = (repo, path, schema)
                    except Exception as error:
                        self.schema_status['schemas_failed'] += int(repo is self.repository)
                        self.add('invalid_schema', repo, path, str(error))
        def retrieve(uri):
            # Registered $ids and local files only; referencing never fetches HTTP.
            parsed = urlsplit(uri)
            if parsed.scheme == 'file' and parsed.netloc in {'', 'localhost'}:
                uri = self.resolve_path(Path(url2pathname(parsed.path))).as_uri()
            if uri in schemas:
                return Resource.from_contents(schemas[uri][2], default_specification=DRAFT7)
            raise ValueError(f'Reference is not a tracked local schema (network access disabled): {uri}')
        registry = Registry(retrieve=retrieve)
        for uri, (_, _, schema) in schemas.items():
            resource = Resource.from_contents(schema, default_specification=DRAFT7)
            registry = registry.with_resource(uri, resource)
            if resource.id():
                registry = registry.with_resource(urljoin(uri, resource.id()), resource)
        validators = {}
        for uri, (repo, path, schema) in schemas.items():
            try:
                cls = validator_for(schema)
                cls.check_schema(schema)
                self.schema_status['schemas_checked'] += int(repo is self.repository)
                # Enter through the effective canonical ID, or the retrieval
                # URI when absent. Draft-07 root $ref schemas ignore sibling
                # $id, so use the dialect's Resource.id() rather than inject one.
                resource = Resource.from_contents(schema, default_specification=DRAFT7)
                canonical = urljoin(uri, resource.id() or '')
                validators[uri] = cls({'$ref': canonical}, registry=registry)
                if repo is not self.repository:
                    continue
                for base, ref in schema_references(schema, uri):
                    self.schema_status['references_checked'] += 1
                    try:
                        registry.resolver(base).lookup(ref)
                    except Exception as error:
                        resolved = urlsplit(urljoin(base, ref))
                        resource_uri = resolved._replace(fragment='').geturl()
                        unavailable_external = resolved.scheme not in {'', 'file'} and resource_uri not in registry
                        self.add('unresolved_schema_reference', repo, path,
                                 f'Cannot resolve {ref!r} offline: {error}. External/unregistered references need manual checking.',
                                 kind='review' if unavailable_external else 'issue')
            except Exception as error:
                self.add('invalid_schema', repo, path, str(error))
        self.schema_status['status'] = 'completed'
        for repo in self.repositories:
            for path in repo.text:
                suffix = Path(path).suffix
                if suffix not in FORMATS and not path.endswith('tuning-overrides.json'):
                    continue
                document = self.parse_json(repo, path)
                if document is INVALID_JSON:
                    self.schema_status['documents_unvalidated'] += 1
                    continue
                reference = document.get('$schema') if isinstance(document, dict) else None
                schema_path = None
                if isinstance(reference, str) and not urlsplit(reference).scheme:
                    schema_path = self.resolve_path((repo.root / path).parent / reference)
                    if not schema_path.exists():
                        self.add('document_schema_link_review', repo, path,
                                 f'Declared schema path {reference!r} does not resolve. Relocated parity fixtures may '
                                 'intentionally preserve their original metadata.', kind='review')
                        schema_path = None
                if schema_path is None and suffix in FORMATS:
                    schema_path = self.schema_repository.root / 'schemas' / (FORMATS[suffix] + '.schema.json')
                uri = schema_path.as_uri() if schema_path else reference
                validator = validators.get(uri)
                if validator is None:
                    self.schema_status['documents_unvalidated'] += 1
                    self.add('document_schema_unavailable', repo, path, 'No usable tracked schema was found for this document.', kind='review')
                    continue
                try:
                    errors = list(validator.iter_errors(document))
                    self.schema_status['documents_checked'] += 1
                    if errors:
                        self.schema_status['documents_failed'] += 1
                        self.add('invalid_document', repo, path,
                                 'Document fails its authoring schema. Compare Rust loader defaults before deciding '
                                 'whether the document or schema needs correction.',
                                 errors=[{'path': list(e.absolute_path), 'message': e.message} for e in errors])
                except Exception as error:
                    self.schema_status['documents_unvalidated'] += 1
                    self.add('document_validation_unavailable', repo, path, f'Validation could not finish: {error}', kind='review')

    def report(self):
        return {
            'format_version': 2, 'generated_at': datetime.now(timezone.utc).isoformat(),
            'repositories': [{'name': r.name, 'root': str(r.root), 'head': r.head,
                              'tracked_entries': len(r.files), 'tracked_changes': r.status,
                              'skipped_paths': r.skipped} for r in self.repositories],
            'reference_context': [str(r.root) for r in self.context if r is not self.repository],
            'scope': 'Tracked working-tree files; static checks and optional JSON Schema validation. '
                     'No repository commands other than read-only Git queries were executed.',
            'private_repositories': self.private_repositories,
            'findings': sorted(self.findings, key=lambda x: (x['kind'] != 'issue', x['rule'], x['repository'], x['path'], x.get('line', 0))),
            'schema_validation': self.schema_status, 'statistics': dict(self.stats),
            'tests': self.tests, 'manual_review': MANUAL_REVIEW,
            'limits': ['Node tests are source declarations; parameterized cases are not expanded. Rust cfg gates and GPU skips are not executed.',
                       'Markdown extraction handles inline links and reference definitions, with heuristic heading anchors; it is not a full Markdown parser.',
                       'Package checks handle literal Node/Python targets and literal bin/exports paths; shell expansion and command flags are not interpreted.',
                       'Schema examples are checked against JSON schemas, not executed through Rust loaders. Missing optional validation is reported as skipped.',
                       'No findings is not proof that every test, config, dependency, or script remains useful.'],
        }


def line_at(text, offset):
    return text.count('\n', 0, offset) + 1


def line_of(text, needle):
    return line_at(text, max(0, text.find(needle)))


def walk_string_values(value, prefix=''):
    if isinstance(value, str):
        yield prefix, value
    elif isinstance(value, dict):
        for key, child in value.items():
            yield from walk_string_values(child, prefix + '/' + key)
    elif isinstance(value, list):
        for i, child in enumerate(value):
            yield from walk_string_values(child, prefix + '/' + str(i))


def schema_references(value, base):
    if isinstance(value, dict):
        base = urljoin(base, value.get('$id', ''))
        if '$ref' in value:
            yield base, value['$ref']
        for child in value.values():
            yield from schema_references(child, base)
    elif isinstance(value, list):
        for child in value:
            yield from schema_references(child, base)


def markdown_anchors(text):
    anchors = set(re.findall(r'\b(?:id|name)=["\']([^"\']+)["\']', text))
    seen = Counter()
    for match in re.finditer(r'(?m)^#{1,6}\s+(.+?)\s*#*\s*$', text):
        title = re.sub(r'<[^>]*>', '', match[1]).lower()
        slug = re.sub(r'[^\w\- ]', '', title).replace(' ', '-')
        anchors.add(slug + (f'-{seen[slug]}' if seen[slug] else ''))
        seen[slug] += 1
    return anchors


def mask_rust(text):
    """Mask strings/comments while retaining offsets, newlines, and structural braces."""
    output = list(text)
    i = 0
    while i < len(text):
        end = i
        if text.startswith('//', i):
            end = text.find('\n', i)
            end = len(text) if end < 0 else end
        elif text.startswith('/*', i):
            depth, end = 1, i + 2
            while end < len(text) and depth:
                if text.startswith('/*', end):
                    depth += 1
                    end += 2
                elif text.startswith('*/', end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
        else:
            raw = re.match(r'(?:br|r)(#*)"', text[i:]) if text[i] in 'br' else None
            char = re.match(r"'(?:\\.|[^'\\\n])'", text[i:]) if text[i] == "'" else None
            if raw:
                close = '"' + raw[1]
                found = text.find(close, i + len(raw[0]))
                end = len(text) if found < 0 else found + len(close)
            elif char:
                end = i + len(char[0])
            elif text[i] == '"':
                end = i + 1
                while end < len(text):
                    if text[end] == '\\':
                        end += 2
                    elif text[end] == '"':
                        end += 1
                        break
                    else:
                        end += 1
        if end > i:
            output[i:end] = ['\n' if c == '\n' else ' ' for c in text[i:end]]
            i = end
        else:
            i += 1
    return ''.join(output)
