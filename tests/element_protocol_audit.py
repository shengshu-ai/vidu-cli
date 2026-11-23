"""Compare original and switched element contracts through real CLI binaries.

Only a deterministic local HTTP fixture is used. No real credentials or server
writes occur. Request traces include methods, complete queries and request bytes;
JSON output is compared structurally because object member order is not a contract.
"""
import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import struct
import subprocess
import threading
import urllib.parse
import zlib


def png():
    """Generate a valid, nontrivial RGB PNG without third-party dependencies."""
    rows = b"".join(b"\0" + bytes((x * 17 + y * 29 + c * 43) % 256 for x in range(256) for c in range(3)) for y in range(256))
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 256, 256, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b'')


class Fixture(http.server.ThreadingHTTPServer):
    """Record requests and emulate deterministic original/V2 service contracts."""
    def __init__(self):
        super().__init__(('127.0.0.1', 0), Handler)
        self.base = 'http://127.0.0.1:%s' % self.server_port
        self.trace = []
        self.options = {}
        self.image = png()


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self): self.handle_fixture()
    def do_POST(self): self.handle_fixture()
    def do_PUT(self): self.handle_fixture()
    def do_DELETE(self): self.handle_fixture()

    def handle_fixture(self):
        server = self.server
        url = urllib.parse.urlsplit(self.path)
        raw = self.rfile.read(int(self.headers.get('Content-Length', '0')))
        try: body = json.loads(raw) if raw else None
        except (ValueError, UnicodeDecodeError): body = {'byte_length': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}
        server.trace.append({'method': self.command, 'path': url.path, 'query': sorted(urllib.parse.parse_qsl(url.query, keep_blank_values=True)), 'body': body, 'auth': self.headers.get('Authorization'), 'content_type': self.headers.get('Content-Type')})
        path = url.path
        status = 200
        element = {'id': '42', 'version': '9007199254740993', 'name': '测试主体', 'modality': 'image', 'type': 'user', 'edition': 2, 'allowed_usages': ['agent'], 'labels': [{'id': '7'}], 'tag_ids': ['1'], 'recaption': {'style': '写实', 'description': '旧描述'}, 'components': [{'asset_id': '91', 'content': 'https://preview.invalid/image.png', 'type': 'main'}]}
        if server.options.get('fail_path') == path:
            status = 403
            response = {'reason': 'ELEMENT_FORBIDDEN', 'message': 'mock permission denied', 'trace_id': 'audit-fixture'}
        elif path == '/image.png':
            response = server.image
        elif path == '/blob':
            response = {}
        elif path == '/tools/v1/files/uploads':
            response = {'id': '123', 'put_url': server.base + '/blob'}
        elif path == '/tools/v1/files/uploads/123/finish':
            response = {}
        elif path == '/tools/v1/assets/uploads':
            response = {'upload_token': 'fixture-upload', 'put_url': server.base + '/blob'}
        elif path == '/tools/v1/assets/uploads/complete':
            response = {'upload_uri': 'ssupload:?id=123'}
        elif path.endswith('/create/check'):
            response = {'exists': False}
        elif path.endswith('/pre-process'):
            response = {'id': '42', 'creator_id': '7', 'recaption': {'style': '写实', 'description': '自动描述'}}
        elif path.endswith('/elements/create') or (path.endswith('/elements') and self.command == 'POST'):
            response = {'id': '42', 'version': '9007199254740993'}
        elif '/elements/detail/' in path:
            response = {'element': element, 'share': {'id': '84'}, 'trace_id': 'audit-fixture'}
            if server.options.get('legacy_edition'): response['element']['edition'] = 1
            if server.options.get('no_share'): response.pop('share')
            if server.options.get('bad_caption'): response['element']['recaption'] = None
        elif path.endswith('/elements/personal') or path.endswith('/elements/official'):
            query = dict(urllib.parse.parse_qsl(url.query, keep_blank_values=True))
            token = query.get('pager.page_token', query.get('query.pager.page_token', ''))
            response = {'elements': [] if server.options.get('empty') else [element], 'next_page_token': 'next' if not token or server.options.get('cycle') else ''}
        elif path.endswith('/share_elements/feed'):
            response = {'share_elements': [] if server.options.get('empty') else [{'element': element, 'share': {'id': '84', 'category_display': ['角色']}}], 'next_page_token': ''}
        elif path.endswith('/elements/tags'):
            response = {'tags': [{'id': '1', 'display_name_zh': '角色', 'display_name_en': 'Character', 'rank': 5, 'extra': {'element': {'selectable': True}}}]}
            if server.options.get('extra_tag_names'): response = {'tags': [{'id': '1', 'display_name_zh': '角色', 'display_name_en': 'Character', 'name': '忽略', 'name_en': 'Ignored'}]}
        elif '/elements/42' in path or '/votes/' in path:
            response = element if self.command == 'PUT' and '/elements/' in path else {}
        else:
            status = 500
            response = {'message': 'unexpected fixture path: ' + path}
        raw_response = response if isinstance(response, bytes) else json.dumps(response, ensure_ascii=False).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'image/png' if isinstance(response, bytes) else 'application/json')
        self.send_header('Content-Length', str(len(raw_response)))
        self.send_header('ETag', 'audit-etag')
        self.end_headers()
        self.wfile.write(raw_response)


def run(binary, mode, args, fixture, options=None):
    """Use the same argv[0] and server for a faithful command-output comparison."""
    fixture.trace = []
    fixture.options = options or {}
    env = dict(os.environ, VIDU_BASE_URL=fixture.base, VIDU_TOKEN='local-audit-fixture')
    for key in ['VIDU_DEBUG', 'VIDU_ELEMENT_PLATFORM_ASSETS', 'VIDU_ELEMENT_PROTOCOL']:
        env.pop(key, None)
    if mode is not None: env['VIDU_ELEMENT_PLATFORM_ASSETS'] = mode
    if fixture.options.get('debug'): env['VIDU_DEBUG'] = '1'
    process = subprocess.run(['vidu-cli', *args], executable=str(binary), env=env, text=True, capture_output=True, timeout=15)
    try: stdout = json.loads(process.stdout)
    except ValueError: stdout = process.stdout
    return {'exit_code': process.returncode, 'stdout': stdout, 'stderr': process.stderr, 'requests': list(fixture.trace)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--main', required=True, type=Path)
    parser.add_argument('--current', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    fixture = Fixture()
    thread = threading.Thread(target=fixture.serve_forever, daemon=True)
    thread.start()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    image = args.output.parent / 'audit-image.png'
    image.write_bytes(fixture.image)
    # Every original subcommand is exercised for help and real service dispatch.
    actions = ['get', 'update', 'delete', 'favorites', 'favorite', 'unfavorite', 'check', 'preprocess', 'create', 'list', 'search']
    cases = [('help-element', ['element', '--help'], {})]
    cases += [('help-' + a, ['element', a, '--help'], {}) for a in actions]
    cases += [
        ('get', ['element', 'get', '42'], {}),
        ('get-table', ['element', 'get', '42', '--format', 'table'], {}),
        ('get-debug', ['element', 'get', '42'], {'debug': True}),
        ('check', ['element', 'check', '--name', '测试主体'], {}),
        ('preprocess', ['element', 'preprocess', '--name', '测试主体', '--image', 'ssupload:?id=123'], {}),
        ('create-uri', ['element', 'create', '--name', '测试主体', '--image', 'ssupload:?id=123'], {}),
        ('create-three', ['element', 'create', '--name', '测试主体', '--image', 'ssupload:?id=123', '--image', 'ssupload:?id=124', '--image', 'ssupload:?id=125', '--style', '写实', '--description', '新描述'], {}),
        ('create-local', ['element', 'create', '--name', '测试主体', '--image', str(image)], {}),
        ('create-http', ['element', 'create', '--name', '测试主体', '--image', fixture.base + '/image.png'], {}),
        ('update-name', ['element', 'update', '42', '--name', '新名字'], {}),
        ('update-labels', ['element', 'update', '42', '--label', '7', '--label', '8'], {}),
        ('update-clear-labels', ['element', 'update', '42', '--clear-labels'], {}),
        ('update-description', ['element', 'update', '42', '--recaption', '新描述'], {}),
        ('update-combined', ['element', 'update', '42', '--name', '新名字', '--label', '8', '--recaption', '新描述'], {}),
        ('delete', ['element', 'delete', '42'], {}),
        ('favorites', ['element', 'favorites'], {}),
        ('favorites-empty', ['element', 'favorites', '--pagesz', '1', '--page-token', 'next'], {'empty': True}),
        ('favorite', ['element', 'favorite', '42'], {}),
        ('unfavorite', ['element', 'unfavorite', '42'], {}),
        ('list', ['element', 'list'], {}),
        ('list-keyword', ['element', 'list', '--keyword', '角色', '--pagesz', '100'], {}),
        ('list-page', ['element', 'list', '--page', '1'], {}),
        ('list-cursor', ['element', 'list', '--page-token', 'next'], {}),
        ('list-past-end', ['element', 'list', '--page', '9'], {}),
        ('list-empty', ['element', 'list'], {'empty': True}),
        ('list-cycle', ['element', 'list', '--page', '2'], {'cycle': True}),
        ('search', ['element', 'search', '--keyword', '男生'], {}),
        ('search-filters', ['element', 'search', '--keyword', '男生', '--category', 'role', '--topic', 'summer', '--sort-by', 'time', '--pagesz', '100', '--page-token', 'next'], {}),
        ('search-empty', ['element', 'search', '--keyword', '男生'], {'empty': True}),
        ('favorite-no-share', ['element', 'favorite', '42'], {'no_share': True}),
        ('update-bad-caption', ['element', 'update', '42', '--name', '新名字'], {'bad_caption': True}),
    ]
    # Include clap's implicit help command and global-option placement, since
    # dynamically replacing an element schema can otherwise change those paths.
    cases += [
        ('element-no-action', ['element'], {}),
        ('element-help-subcommand', ['element', 'help'], {}),
        ('root-help-element', ['help', 'element'], {}),
        ('short-help-create', ['element', 'create', '-h'], {}),
        ('nested-help-create', ['element', 'help', 'create'], {}),
        ('global-table-before-command', ['--format', 'table', 'element', 'list'], {}),
        ('typo-command', ['element', 'creat'], {}),
        ('invalid-output-format', ['element', 'list', '--format', 'xml'], {}),
    ]
    # Check each service operation's error propagation as well as success.
    for label, command, options in list(cases)[12:]:
        if label in ['get', 'check', 'preprocess', 'create-uri', 'update-name', 'delete', 'favorites', 'favorite', 'list', 'search']:
            trace = run(args.main, None, command, fixture, options)['requests']
            if trace: cases.append((label + '-403', command, {'fail_path': trace[-1]['path']}))
    invalid = [
        ['get', '0'], ['delete', '-1'], ['favorite', 'bad'], ['unfavorite', '0'],
        ['create', '--name', 'test'], ['create', '--name', 'test', '--image', 'missing.png'],
        ['create', '--name', 'test', '--asset-id', '1'], ['create', '--name', 'test', '--tag', '1'],
        ['create', '--name', 'test', '--image', 'ssupload:?id=1', '--image', 'ssupload:?id=2', '--image', 'ssupload:?id=3', '--image', 'ssupload:?id=4'],
        ['update', '42'], ['update', '42', '--label', '7', '--clear-labels'], ['update', '42', '--image', 'missing.png'], ['update', '42', '--label', '0'],
        ['list', '--source', 'official'], ['list', '--page', '1', '--page-token', 'next'], ['list', '--pagesz', '9'],
        ['search'], ['search', '--keyword', ''], ['search', '--tag', '1'], ['search', '--keyword', 'test', '--pagesz', '0'],
        ['favorites', '--pagesz', '101'], ['favorite', '42', '--source', 'official'], ['tags'],
    ]
    cases += [('invalid-%02d' % i, ['element', *command], {}) for i, command in enumerate(invalid)]
    results = []
    for label, command, options in cases:
        baseline = run(args.main, None, command, fixture, options)
        default = run(args.current, None, command, fixture, options)
        off = run(args.current, 'false', command, fixture, options)
        results.append({'name': label, 'args': command, 'options': options, 'default_equal': baseline == default, 'false_equal': baseline == off, 'main': baseline, 'default': default, 'false': off})
    # Independent V2 probes complement the strict Rust service-contract tests.
    probes = []
    commands = [(a + '-help', ['element', a, '--help'], {}) for a in actions + ['tags']]
    commands += [
        ('tags', ['element', 'tags'], {}),
        ('tags-display-contract', ['element', 'tags'], {'extra_tag_names': True}),
        ('get', ['element', 'get', '42'], {}),
        ('create-assets', ['element', 'create', '--name', '新主体', '--tag', '1', '--asset-id', '91', '--asset_id', '92'], {}),
        ('create-local', ['element', 'create', '--name', '新主体', '--tag', '1', '--image', str(image)], {}),
        ('create-http', ['element', 'create', '--name', '新主体', '--tag', '1', '--image', fixture.base + '/image.png'], {}),
        ('update-assets', ['element', 'update', '42', '--asset-id', '92'], {}),
        ('update-local', ['element', 'update', '42', '--image', str(image)], {}),
        ('update-metadata', ['element', 'update', '42', '--name', '新名字', '--recaption', '新描述'], {}),
        ('update-legacy-label', ['element', 'update', '42', '--label', '8'], {'legacy_edition': True}),
        ('delete', ['element', 'delete', '42'], {}),
        ('check', ['element', 'check', '--name', '新主体'], {}),
        ('preprocess', ['element', 'preprocess', '--name', '新主体', '--image', 'ssupload:?id=123'], {}),
        ('list-personal', ['element', 'list'], {}),
        ('list-official', ['element', 'list', '--source', 'official', '--keyword', '角色', '--tag', '1'], {}),
        ('search-community', ['element', 'search', '--category', 'role'], {}),
        ('search-personal', ['element', 'search', '--source', 'personal', '--tag', '1'], {}),
        ('search-official', ['element', 'search', '--source', 'official', '--tag', '1'], {}),
        ('favorites-community', ['element', 'favorites'], {}),
        ('favorites-official', ['element', 'favorites', '--source', 'official', '--tag', '1'], {}),
        ('favorite-community', ['element', 'favorite', '42'], {}),
        ('favorite-official', ['element', 'favorite', '42', '--source', 'official'], {}),
        ('unfavorite-community', ['element', 'unfavorite', '42'], {}),
        ('unfavorite-official', ['element', 'unfavorite', '42', '--source', 'official'], {}),
    ]
    for label, command, options in commands:
        probes.append({'name': label, 'args': command, 'result': run(args.current, 'true', command, fixture, options)})
    # Independently assert the agreed V2 behavior; probes alone are not proof.
    checks = []
    observed = {p['name']: p['result'] for p in probes}
    def check(name, passed, evidence):
        checks.append({'name': name, 'passed': bool(passed), 'evidence': evidence})
    def requests(name): return observed[name]['requests']
    def output(name): return observed[name]['stdout']
    for probe in probes:
        check('command-success:' + probe['name'], probe['result']['exit_code'] == 0, probe['name'])
    routes = {
        'tags': [('GET', '/vidu/v1/material/elements/tags')],
        'get': [('GET', '/vidu/v1/material/elements/detail/42')],
        'create-assets': [('POST', '/vidu/v1/material/elements/create')],
        'create-local': [('POST', '/tools/v1/assets/uploads'), ('PUT', '/blob'), ('PUT', '/tools/v1/assets/uploads/complete'), ('POST', '/vidu/v1/material/elements/create')],
        'create-http': [('GET', '/image.png'), ('POST', '/tools/v1/assets/uploads'), ('PUT', '/blob'), ('PUT', '/tools/v1/assets/uploads/complete'), ('POST', '/vidu/v1/material/elements/create')],
        'update-assets': [('GET', '/vidu/v1/material/elements/detail/42'), ('PUT', '/vidu/v2/material/elements/42')],
        'update-local': [('GET', '/vidu/v1/material/elements/detail/42'), ('POST', '/tools/v1/assets/uploads'), ('PUT', '/blob'), ('PUT', '/tools/v1/assets/uploads/complete'), ('PUT', '/vidu/v2/material/elements/42')],
        'update-metadata': [('GET', '/vidu/v1/material/elements/detail/42'), ('PUT', '/vidu/v2/material/elements/42')],
        'update-legacy-label': [('GET', '/vidu/v1/material/elements/detail/42'), ('PUT', '/vidu/v1/material/elements/42')],
        'delete': [('DELETE', '/vidu/v1/material/elements/42')],
        'check': [('GET', '/vidu/v1/material/elements/create/check')],
        'preprocess': [('POST', '/vidu/v1/material/elements/pre-process')],
        'list-personal': [('GET', '/vidu/v1/material/elements/personal')],
        'list-official': [('GET', '/vidu/v1/material/elements/official')],
        'search-community': [('GET', '/vidu/v1/material/share_elements/feed')],
        'search-personal': [('GET', '/vidu/v1/material/elements/personal')],
        'search-official': [('GET', '/vidu/v1/material/elements/official')],
        'favorites-community': [('GET', '/vidu/v1/material/share_elements/feed')],
        'favorites-official': [('GET', '/vidu/v1/material/elements/official')],
        'favorite-community': [('GET', '/vidu/v1/material/elements/detail/42'), ('PUT', '/tools/v1/votes/share_element/84')],
        'favorite-official': [('PUT', '/tools/v1/votes/official_element/42')],
        'unfavorite-community': [('GET', '/vidu/v1/material/elements/detail/42'), ('DELETE', '/tools/v1/votes/share_element/84')],
        'unfavorite-official': [('DELETE', '/tools/v1/votes/official_element/42')],
    }
    for name, route in routes.items():
        actual = [(r['method'], r['path']) for r in requests(name)]
        check('route:' + name, actual == route, actual)
    check('first-level-tags-query', requests('tags')[0]['query'] == [('parent_id', '0')], requests('tags'))
    check('tag-projection', output('tags')['tags'] == [{'id':'1','name':'角色','name_en':'Character'}], output('tags'))
    check('display-tag-names', output('tags-display-contract')['tags'] == [{'id':'1','name':'角色','name_en':'Character'}], output('tags-display-contract'))
    check('single-create-tag', requests('create-assets')[0]['body']['tag_ids'] == ['1'], requests('create-assets')[0]['body'])
    check('ordered-asset-references', requests('create-assets')[0]['body']['components'] == [{'asset_id':'91','type':'main','content_type':'image'}, {'asset_id':'92','type':'auxiliary','content_type':'image'}], requests('create-assets')[0]['body'])
    check('create-id-version-precision', output('create-assets') == {'ok':True,'id':'42','version':'9007199254740993'}, output('create-assets'))
    for name in ['create-local', 'create-http']:
        trace = requests(name)
        check('asset-scene:' + name, next(r for r in trace if r['path'] == '/tools/v1/assets/uploads')['body']['scene'] == 'material', trace)
        check('upload-uri:' + name, trace[-1]['body']['components'][0]['content'] == 'ssupload:?id=123', trace[-1]['body'])
    patch = requests('update-assets')[-1]['body']
    check('replace-assets-mask', patch['mask'] == 'components' and patch['element']['components'] == [{'asset_id':'92','type':'main','content_type':'image'}], patch)
    patch = requests('update-metadata')[-1]['body']
    check('preserve-style-and-taxonomy', patch['mask'] == 'name,recaption' and patch['element']['recaption'] == {'style':'写实','description':'新描述'} and 'tag_ids' not in patch['element'], patch)
    for name in ['get', 'update-assets', 'update-local', 'update-metadata', 'update-legacy-label']:
        data = output(name)
        element = data.get('element', data)
        check('omit-allowed-usages:' + name, 'allowed_usages' not in data and 'allowed_usages' not in element, data)
        check('preserve-assets-and-uri:' + name, element['components'][0]['asset_id'] == '91' and element['components'][0]['content'] == 'https://preview.invalid/image.png', element['components'])
    for name in ['get', 'update-assets', 'update-local', 'update-metadata', 'update-legacy-label']:
        data = output(name)
        check('omit-edition:' + name, 'edition' not in data and 'edition' not in data.get('element', data), data)
    for name in ['list-personal', 'search-personal']:
        check('personal-without-preset:' + name, dict(requests(name)[0]['query']).get('without_preset') == 'true', requests(name)[0]['query'])
    for name in ['list-personal', 'list-official', 'search-personal', 'search-official', 'favorites-official']:
        data = output(name)
        check('uniform-summary:' + name, set(data['elements'][0]) == {'id','version','name'} and 'library_source' not in json.dumps(data), data)
    for name in ['search-personal', 'search-official']:
        query = dict(requests(name)[0]['query'])
        check('tag-filter:' + name, query.get('tag_ids', query.get('query.tag_ids')) == '1', query)
    for name in ['favorites-community', 'favorites-official']:
        query = dict(requests(name)[0]['query'])
        check('collection-filter:' + name, query.get('is_collect', query.get('collected_only')) == 'true', query)
    for name in ['favorite-official', 'unfavorite-official']:
        check('official-does-not-get-detail:' + name, len(requests(name)) == 1, requests(name))
    for name in ['create-help', 'update-help']:
        help_text = output(name)
        check('hidden-switch:' + name, '--platform-assets' not in help_text and 'VIDU_ELEMENT_PLATFORM_ASSETS' not in help_text, help_text)
    check('create-help-contract', '--tag' in output('create-help') and '--asset-id' in output('create-help') and '--style' not in output('create-help'), output('create-help'))
    check('update-immutable-tag', '--tag' not in output('update-help'), output('update-help'))
    report = {'baseline_binary_sha256': hashlib.sha256(args.main.read_bytes()).hexdigest(), 'current_binary_sha256': hashlib.sha256(args.current.read_bytes()).hexdigest(), 'differential': results, 'v2_probes': probes, 'v2_checks': checks}
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    fixture.shutdown()
    fixture.server_close()
    failures = [item['name'] for item in results if not item['default_equal'] or not item['false_equal']]
    print(json.dumps({'differential_cases': len(results), 'default_pass': sum(x['default_equal'] for x in results), 'false_pass': sum(x['false_equal'] for x in results), 'failures': failures, 'v2_probes': len(probes), 'v2_checks': len(checks), 'v2_check_failures': [c['name'] for c in checks if not c['passed']]}, ensure_ascii=False))
    return bool(failures) or any(not item["passed"] for item in checks)


if __name__ == '__main__':
    raise SystemExit(main())
