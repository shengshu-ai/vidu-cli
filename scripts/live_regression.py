#!/usr/bin/env python3
"""Read-only live CLI regression; secrets stay in child environment, raw data in /tmp.

Run from any directory: python3 scripts/live_regression.py --read-only
No uploads, submissions, mutations, downloads, or external Python dependencies.
Successful CLI responses do not expose HTTP status; report it as null, never infer 200.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import urllib.parse


def dotenv(path):
    values = {}
    for number, line in enumerate(path.read_text().splitlines(), 1):
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        if line.startswith('export '):
            line = line[7:].lstrip()
        match = re.fullmatch(r'([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)', line)
        if not match:
            raise ValueError(f'invalid dotenv assignment at line {number}')
        key, value = match.groups()
        if value.startswith(('"', "'")):
            quote = value[0]
            # No expansion, shell evaluation, or interpolation of dotenv contents.
            end = 1
            while end < len(value):
                if value[end] == quote and (quote == "'" or value[end - 1] != '\\'):
                    break
                end += 1
            tail = value[end + 1:].strip()
            if end == len(value) or (tail and not tail.startswith('#')):
                raise ValueError(f'invalid dotenv quoting at line {number}')
            value = value[1:end]
            if quote == '"':
                value = value.replace('\\"', '"').replace('\\\\', '\\')
        else:
            value = re.split(r'\s+#', value, maxsplit=1)[0].rstrip()
        values[key] = value
    return values


def structure(value, depth=0):
    if isinstance(value, dict):
        return {k: structure(v, depth + 1) for k, v in value.items()} if depth < 3 else {'type': 'object', 'keys': sorted(value)}
    if isinstance(value, list):
        return {'type': 'array', 'count': len(value), 'item': structure(value[0], depth + 1) if value and depth < 3 else None}
    return type(value).__name__


def objects(value):
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from objects(child)
    elif isinstance(value, list):
        for child in value:
            yield from objects(child)


def redact_strings(value, secret):
    """Redact credentials in decoded data and common JSON/URL log encodings.

    Match only representations of the supplied credential: signed media URLs
    must remain usable for downloads and recovery in private regression state.
    """
    if isinstance(value, str):
        if not secret:
            return value
        variants = {secret}
        for encoder in (urllib.parse.quote, urllib.parse.quote_plus):
            encoded = encoder(secret, safe='')
            variants.add(encoded)
            variants.add(re.sub(r'%[0-9A-F]{2}', lambda m: m.group().lower(), encoded))
        for variant in tuple(variants):
            for ensure_ascii in (True, False):
                encoded = json.dumps(variant, ensure_ascii=ensure_ascii)[1:-1]
                variants.update((encoded, encoded.replace('/', r'\/')))
        for variant in sorted(variants, key=len, reverse=True):
            value = value.replace(variant, '[REDACTED]')
        return value
    if isinstance(value, dict):
        return {redact_strings(key, secret): redact_strings(child, secret) for key, child in value.items()}
    if isinstance(value, list):
        return [redact_strings(child, secret) for child in value]
    return value


def result_exit_code(results):
    """A skipped case is neutral; any command/semantic failure makes the CLI fail."""
    return int(any(row.get('ok') is False for row in results))


def semantic_results(responses):
    """Evaluate saved successful responses without network or vacuous empty passes."""
    def ids(case, field):
        return [str(item['id']) for item in responses[case].get(field, [])]

    checks = [
        ('task_filters_respected', [('task_list_repeated_filters', 'tasks')], lambda: all(t['state'] in ('success', 'failed') and t['type'] in ('text2image', 'text2video') for t in responses['task_list_repeated_filters']['tasks'])),
        ('task_cursor_disjoint', [('task_list', 'tasks'), ('task_list_cursor', 'tasks')], lambda: not set(ids('task_list', 'tasks')).intersection(ids('task_list_cursor', 'tasks'))),
        ('element_page_cursor_equivalent', [('element_list_page1', 'elements'), ('element_list_cursor', 'elements')], lambda: ids('element_list_page1', 'elements') == ids('element_list_cursor', 'elements')),
        ('quota_pages_disjoint', [('quota_records_0', 'records'), ('quota_records_1', 'records')], lambda: not set(ids('quota_records_0', 'records')).intersection(ids('quota_records_1', 'records'))),
        ('task_details_preserve_state', [('task_get', None), ('task_get_details', None)], lambda: bool(responses['task_get'].get('state')) and responses['task_get'].get('state') == responses['task_get_details'].get('state') and isinstance(responses['task_get_details'].get('input'), dict) and isinstance(responses['task_get_details'].get('settings'), dict) and isinstance(responses['task_get_details'].get('creations'), list)),
    ]
    for name, prerequisites, check in checks:
        if not all(case in responses for case, _ in prerequisites):
            yield {'name': name, 'skipped': True, 'reason': 'successful_prerequisite_unavailable'}
        elif any(field is not None and not responses[case].get(field) for case, field in prerequisites):
            yield {'name': name, 'skipped': True, 'reason': 'no_records_to_validate'}
        else:
            try:
                passed = check()
            except (KeyError, TypeError, ValueError):
                passed = False
            yield {'name': name, 'ok': bool(passed), 'reason': 'semantic_check_passed' if passed else 'semantic_check_failed'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--read-only', action='store_true', default=True)
    parser.add_argument('--env-file', type=Path)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--timeout', type=int, default=65)
    parser.add_argument('--category', help='Verified server category code for an additional search regression')
    parser.add_argument('--topic', help='Verified server topic code for an additional search regression')
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    binary = (args.binary or repo / 'target/release/vidu-cli').resolve()
    source_paths = [*repo.glob('src/**/*.rs'), *[repo / f for f in ('Cargo.toml', 'Cargo.lock', 'build.rs')]]
    if any(p.exists() and p.stat().st_mtime > binary.stat().st_mtime for p in source_paths):
        raise SystemExit('release binary is stale; build it before live regression')
    config = dotenv(args.env_file or repo / '.env')
    if not config.get('VIDU_TOKEN') or not config.get('VIDU_BASE_URL', '').startswith('https://'):
        raise SystemExit('dotenv must contain VIDU_TOKEN and an HTTPS VIDU_BASE_URL')
    env = dict(os.environ, **config)
    env.pop('VIDU_DEBUG', None)
    os.umask(0o077)
    output = Path(tempfile.mkdtemp(prefix='vidu-live-regression-', dir='/private/tmp'))
    results = []
    responses = {}

    def save(name, payload):
        path = output / name
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, 'w') as stream:
            json.dump(payload, stream, ensure_ascii=False, indent=2)

    def record(row):
        results.append(row)
        print(json.dumps(row, ensure_ascii=False), flush=True)

    def skip(name, reason):
        record({'name': name, 'skipped': True, 'reason': reason})

    def run(name, command, plain=False):
        # Explicit allowlist protects subsequent extensions from accidental writes.
        cmd = command[2:] if command[:2] == ['--format', 'table'] else command
        allowed = {'quota': {'pass', 'credit', 'records'}, 'task': {'list', 'get', 'wait', 'cost', 'tts-cost', 'lip-sync-cost', 'tts-voices', 'lip-sync-voices'}, 'element': {'list', 'get', 'search', 'favorites'}, 'creation': {'list'}, 'voice': {'list', 'get'}}
        if '--help' not in cmd and not (len(cmd) > 1 and cmd[1] in allowed.get(cmd[0], set())):
            raise ValueError('command is outside read-only allowlist')
        if '--output' in cmd or '-o' in cmd:
            raise ValueError('downloads are forbidden')
        started = time.monotonic()
        try:
            proc = subprocess.run([str(binary), *command], env=env, capture_output=True, text=True, timeout=args.timeout)
            raw = {'args': command, 'exit_code': proc.returncode, 'stdout': proc.stdout, 'stderr': proc.stderr}
        except subprocess.TimeoutExpired:
            record({'name': name, 'exit_code': None, 'ok': False, 'http_status': None, 'reason': 'runner_timeout'})
            return {}
        # Defense in depth: even restricted raw files never retain the token.
        try:
            payload = redact_strings(json.loads(raw['stdout']), config['VIDU_TOKEN'])
        except ValueError:
            payload = {}
        raw = redact_strings(raw, config['VIDU_TOKEN'])
        save(name + '.json', raw)
        error = payload.get('error', {})
        reason = error.get('type', 'ok' if proc.returncode == 0 else 'non_json_failure')
        row = {'name': name, 'exit_code': proc.returncode, 'ok': proc.returncode == 0 and (plain or payload.get('ok') is True), 'http_status': error.get('http_status'), 'reason': reason, 'trace_id': error.get('trace_id') or payload.get('trace_id'), 'elapsed_seconds': round(time.monotonic() - started, 2), 'structure': structure(payload) if payload else {'text_lines': len(raw['stdout'].splitlines())}}
        # Only stable diagnostic categories/codes, never arbitrary server messages.
        if error.get('code') and re.fullmatch(r'[A-Za-z0-9_.:-]{1,100}', str(error['code'])):
            row['error_code'] = error['code']
        if 'cost' in name and proc.returncode == 0:
            row['cost_numbers'] = {k: v for obj in objects(payload) for k, v in obj.items() if isinstance(v, (int, float)) and not isinstance(v, bool) and 'cost' in k}
        record(row)
        data = payload.get('data', payload) if isinstance(payload, dict) else {}
        if row['ok'] and not plain:
            responses[name] = data
        return data

    run('help', ['--help'], True)
    for action in ('pass', 'credit'):
        run('quota_' + action, ['quota', action])
    for page in ('0', '1'):
        run('quota_records_' + page, ['quota', 'records', '--page', page, '--pagesz', '10'])
    tasks = run('task_list', ['task', 'list', '--pagesz', '10'])
    run('task_list_repeated_filters', ['task', 'list', '--state', 'success', '--state', 'failed', '--type', 'text2image', '--type', 'text2video', '--pagesz', '10'])
    token = tasks.get('next_page_token')
    if token:
        run('task_list_cursor', ['task', 'list', '--pagesz', '10', '--page-token', str(token)])
    else:
        skip('task_list_cursor', 'no_returned_cursor')
    candidates = [obj for obj in objects(tasks) if obj.get('state') and (obj.get('id') or obj.get('task_id'))]
    terminal = [obj for obj in candidates if obj['state'] in ('success', 'failed', 'canceled')]
    chosen = next((obj for obj in terminal if obj['state'] == 'success'), next(iter(terminal or candidates), None))
    if chosen:
        task_id = str(chosen.get('id') or chosen['task_id'])
        run('task_get', ['task', 'get', task_id])
        run('task_get_details', ['task', 'get', task_id, '--details'])
        if chosen in terminal:
            run('task_wait_terminal', ['task', 'wait', task_id, '--timeout', '15', '--poll-interval', '1'])
        else:
            skip('task_wait_terminal', 'no_terminal_task_in_history')
    else:
        skip('task_get_get_details_wait', 'no_owned_task_in_history')
    elements = run('element_list', ['element', 'list', '--pagesz', '10'])
    run('element_list_page0', ['element', 'list', '--page', '0', '--pagesz', '10'])
    run('element_list_page1', ['element', 'list', '--page', '1', '--pagesz', '10'])
    owned = next((obj for obj in objects(elements) if obj.get('id')), None)
    if owned:
        run('element_get', ['element', 'get', str(owned['id'])])
    if elements.get('next_page_token'):
        run('element_list_cursor', ['element', 'list', '--pagesz', '10', '--page-token', elements['next_page_token']])
    else:
        skip('element_list_cursor', 'no_returned_cursor')
    keyword = str(owned.get('name') or '猫') if owned else '猫'
    run('element_list_keyword', ['element', 'list', '--keyword', keyword, '--pagesz', '10'])
    search = run('element_search', ['element', 'search', '--keyword', '猫', '--pagesz', '10'])
    run('element_search_pagesz1', ['element', 'search', '--keyword', '猫', '--pagesz', '1'])
    if search.get('next_page_token'):
        run('element_search_cursor', ['element', 'search', '--keyword', '猫', '--pagesz', '10', '--page-token', search['next_page_token']])
    # Dynamic taxonomy comes from verified server metadata supplied by the caller.
    for flag, value in [('category', args.category), ('topic', args.topic)]:
        if value:
            run('element_search_' + flag, ['element', 'search', '--keyword', '猫', '--pagesz', '10', '--' + flag, value])
        else:
            skip('element_search_' + flag, 'no_verified_filter_code_supplied')
    run('element_favorites', ['element', 'favorites', '--pagesz', '10'])
    run('element_favorites_pagesz1', ['element', 'favorites', '--pagesz', '1'])
    run('creation_list', ['creation', 'list', '--pagesz', '10'])
    run('creation_favorites', ['creation', 'list', '--favorites-only', '--pagesz', '10'])
    voices = run('voice_list', ['voice', 'list', '--pagesz', '10'])
    voice = next((obj for obj in objects(voices) if str(obj.get('id') or obj.get('voice_id') or '').startswith('voice_')), None)
    if voice:
        run('voice_get_owned', ['voice', 'get', str(voice.get('id') or voice['voice_id'])])
    else:
        skip('voice_get_owned', 'no_owned_voice_in_list')
    run('E02_table_quota', ['--format', 'table', 'quota', 'credit'], True)
    tts = run('tts_voices', ['task', 'tts-voices'])
    run('lip_sync_voices', ['task', 'lip-sync-voices'])
    run('cost_text2image', ['task', 'cost', '--type', 'text2image', '--model-version', '3.2_image_2_5_fast', '--resolution', '1080p', '--schedule-mode', 'normal'])
    run('cost_text2video', ['task', 'cost', '--type', 'text2video', '--model-version', '3.2', '--duration', '4', '--resolution', '720p', '--transition', 'speed', '--schedule-mode', 'normal'])
    first_voice = next((obj['voice_ids'][0] for obj in objects(tts) if obj.get('voice_ids')), None)
    if first_voice:
        run('cost_tts', ['task', 'tts-cost', '--text', '回归测试', '--voice-id', first_voice, '--schedule-mode', 'normal'])
    else:
        skip('cost_tts', 'no_tts_voice_returned')
    for row in semantic_results(responses):
        record(row)
    save('summary.json', {'read_only': True, 'cases': results, 'binary_mtime_ns': binary.stat().st_mtime_ns})
    print(json.dumps({'output_dir': str(output), 'passed': sum(r.get('ok') is True for r in results), 'failed': sum(r.get('ok') is False for r in results), 'skipped': sum(r.get('skipped') is True for r in results)}, ensure_ascii=False))
    return result_exit_code(results)


if __name__ == '__main__':
    raise SystemExit(main())
