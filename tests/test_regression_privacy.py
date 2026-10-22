"""Offline credential privacy checks; no dotenv file, CLI, or API is accessed."""
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock
import urllib.error
import urllib.parse

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
import live_regression as read_only
import live_write_regression as write_live


SECRET = 'test-"quoted"\\path/猫 +&'
SIGNED_URL = 'https://media.invalid/video.mp4?signature=download-me&expires=123'


def representations(secret):
    variants = {secret, urllib.parse.quote(secret, safe=''), urllib.parse.quote_plus(secret, safe='')}
    for value in tuple(variants):
        for ascii_only in (True, False):
            encoded = json.dumps(value, ensure_ascii=ascii_only)[1:-1]
            variants.update((encoded, encoded.replace('/', r'\/')))
    return variants


class PrivacyTests(unittest.TestCase):
    def setUp(self):
        self.previous_umask = os.umask(0o077)
        self.temp = tempfile.TemporaryDirectory(prefix='vidu-live-write-', dir='/private/tmp')
        self.addCleanup(self.temp.cleanup)
        self.addCleanup(os.umask, self.previous_umask)
        self.root = Path(self.temp.name)
        self.config = {'VIDU_TOKEN': SECRET, 'VIDU_BASE_URL': 'https://api.invalid'}
        self.output = io.StringIO()
        self.enterContext(contextlib.redirect_stdout(self.output))
        self.enterContext(mock.patch.object(write_live, 'dotenv', return_value=self.config))

    def runner(self):
        return write_live.Runner(types.SimpleNamespace(state_dir=self.root, env_file=None, execute=True))

    def assert_private(self, value):
        text = value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)
        for variant in representations(SECRET):
            self.assertNotIn(variant, text)

    def test_recursive_keys_json_and_url_encodings(self):
        values = list(representations(SECRET))
        payload = {SECRET: [{'nested': 'prefix ' + value + ' suffix'} for value in values], 'media': SIGNED_URL}
        cleaned = read_only.redact_strings(payload, SECRET)
        self.assertIn('[REDACTED]', cleaned)
        self.assertEqual(cleaned['media'], SIGNED_URL)
        self.assertTrue(all(item['nested'] == 'prefix [REDACTED] suffix' for item in cleaned['[REDACTED]']))
        self.assertEqual(read_only.redact_strings(payload, ''), payload)
        self.assert_private(cleaned)

    def test_http_success_redacts_before_saving_and_returning(self):
        runner = self.runner()
        payload = {SECRET: {'value': SECRET}, 'uri': SIGNED_URL}
        response = io.StringIO(json.dumps(payload, ensure_ascii=True))
        with mock.patch.object(write_live.urllib.request, 'urlopen', return_value=response) as urlopen:
            result = runner.get('success', '/read', {})
        self.assertEqual(result, {'[REDACTED]': {'value': '[REDACTED]'}, 'uri': SIGNED_URL})
        self.assertEqual(urlopen.call_args.args[0].get_header('Authorization'), 'Token ' + SECRET)
        file = self.root / 'success.json'
        self.assert_private(file.read_text())
        self.assertEqual(file.stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.root.stat().st_mode & 0o777, 0o700)

    def test_http_error_redacts_encoded_body(self):
        runner = self.runner()
        body = json.dumps({'message': SECRET, 'url': urllib.parse.quote_plus(SECRET)}, ensure_ascii=True).replace('/', r'\/')
        failure = urllib.error.HTTPError('https://api.invalid/read', 403, 'Forbidden', {}, io.BytesIO(body.encode()))
        self.addCleanup(failure.close)
        with mock.patch.object(write_live.urllib.request, 'urlopen', side_effect=failure):
            result = runner.get('error', '/read', {})
        self.assertEqual(result['http_status'], 403)
        self.assert_private(result)
        self.assert_private((self.root / 'error.json').read_text())
        self.assertEqual(runner.state['operations']['error']['status'], 'FAIL')

    def test_http_resume_cleans_previous_evidence_without_request(self):
        runner = self.runner()
        runner.state['operations']['cached'] = {'status': 'PASS', 'write': False}
        file = self.root / 'cached.json'
        file.write_text(json.dumps({SECRET: SECRET, 'uri': SIGNED_URL}))
        file.chmod(0o644)
        with mock.patch.object(write_live.urllib.request, 'urlopen') as urlopen:
            result = runner.get('cached', '/read', {})
        urlopen.assert_not_called()
        self.assertEqual(result, {'[REDACTED]': '[REDACTED]', 'uri': SIGNED_URL})
        self.assert_private(file.read_text())
        self.assertEqual(file.stat().st_mode & 0o777, 0o600)

    def test_write_cli_escaped_payload_and_raw_streams(self):
        runner = self.runner()
        payload = {'ok': True, SECRET: {'token': SECRET}, 'uri': SIGNED_URL}
        stdout = json.dumps(payload, ensure_ascii=True).replace('/', r'\/')
        proc = subprocess.CompletedProcess([], 0, stdout, urllib.parse.quote(SECRET, safe=''))
        with mock.patch.object(write_live.subprocess, 'run', return_value=proc) as run:
            result = runner.run('cli', ['task', 'list'])
            cached = runner.run('cli', ['task', 'list'])
        self.assertEqual(run.call_count, 1)
        self.assertEqual(result, cached)
        self.assertEqual(result['[REDACTED]']['token'], '[REDACTED]')
        self.assertEqual(result['uri'], SIGNED_URL)
        saved = json.loads((self.root / 'cli.json').read_text())
        self.assertTrue(json.loads(saved['stdout'])['ok'])
        for file in self.root.glob('*.json'):
            self.assert_private(file.read_text())
        self.assert_private(self.output.getvalue())

    def test_read_only_cli_escaped_payload_logs(self):
        output_dir = self.root / 'readonly'
        output_dir.mkdir()
        binary = self.root / 'fake-cli'
        binary.touch()
        os.utime(binary, (4102444800, 4102444800))
        payload = {'ok': True, 'data': {'tasks': [], SECRET: SECRET, 'uri': SIGNED_URL}}
        stdout = json.dumps(payload, ensure_ascii=True).replace('/', r'\/')
        proc = subprocess.CompletedProcess([], 0, stdout, urllib.parse.quote_plus(SECRET))
        with mock.patch.object(read_only, 'dotenv', return_value=self.config), \
                mock.patch.object(read_only.subprocess, 'run', return_value=proc), \
                mock.patch.object(read_only.tempfile, 'mkdtemp', return_value=str(output_dir)), \
                mock.patch.object(sys, 'argv', ['live_regression.py', '--binary', str(binary)]):
            self.assertEqual(read_only.main(), 0)
        for file in output_dir.glob('*.json'):
            self.assert_private(file.read_text())
        self.assert_private(self.output.getvalue())
        saved = json.loads((output_dir / 'task_list.json').read_text())
        self.assertEqual(json.loads(saved['stdout'])['data']['uri'], SIGNED_URL)


if __name__ == '__main__':
    unittest.main()
