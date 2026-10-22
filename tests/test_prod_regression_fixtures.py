"""Offline checks for caller-supplied production regression media."""
import contextlib
import io
from pathlib import Path
import stat
import sys
import tempfile
import types
import unittest
from unittest.mock import Mock, patch

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
import live_prod_regression as prod


class ProductionFixtureTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.runner = prod.ProductionRunner.__new__(prod.ProductionRunner)
        self.runner.path = self.root / 'state'
        self.runner.path.mkdir(mode=0o700)
        self.runner.args = types.SimpleNamespace(image_fixture=None, audio_fixture=None)
        self.runner.state = {'operations': {}, 'resources': {}}
        self.runner.run = Mock(return_value={})
        self.runner.get = Mock(return_value={})

    def fixture(self, name, content):
        path = self.root / name
        path.write_bytes(content)
        return path

    def test_setup_copies_explicit_fixtures_privately_before_api(self):
        self.runner.args.image_fixture = self.fixture('input.png', b'image bytes')
        self.runner.args.audio_fixture = self.fixture('input.wav', b'audio bytes')

        def check_copies(*args, **kwargs):
            for name, content in [('robot.png', b'image bytes'),
                                  ('generic-synthetic-voice.wav', b'audio bytes')]:
                target = self.runner.path / name
                self.assertEqual(target.read_bytes(), content)
                self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0o600)
            return {}

        self.runner.run.side_effect = check_copies
        self.runner.setup()
        self.assertEqual(stat.S_IMODE(self.runner.path.stat().st_mode), 0o700)
        self.assertEqual(self.runner.run.call_count, 6)

    def test_resume_reuses_existing_copies_without_arguments(self):
        for name in ['robot.png', 'generic-synthetic-voice.wav']:
            (self.runner.path / name).write_bytes(b'original')
        self.runner.setup()
        for name in ['robot.png', 'generic-synthetic-voice.wav']:
            self.assertEqual((self.runner.path / name).read_bytes(), b'original')

    def test_resume_does_not_overwrite_with_new_arguments(self):
        (self.runner.path / 'robot.png').write_bytes(b'original')
        self.runner.args.image_fixture = self.fixture('replacement.png', b'replacement')
        self.runner.prepare_fixtures('image')
        self.assertEqual((self.runner.path / 'robot.png').read_bytes(), b'original')

    def test_missing_fixture_prevents_all_setup_api_calls(self):
        self.runner.args.image_fixture = self.fixture('input.png', b'image')
        with self.assertRaisesRegex(ValueError, '--audio-fixture'):
            self.runner.setup()
        self.runner.run.assert_not_called()
        self.runner.get.assert_not_called()
        self.assertFalse((self.runner.path / 'robot.png').exists())

    def test_missing_fixture_prevents_element_and_voice_creation(self):
        for method, option in [('element', '--image-fixture'), ('voice', '--audio-fixture')]:
            with self.subTest(method=method):
                with self.assertRaisesRegex(ValueError, option):
                    getattr(self.runner, method)()
        self.runner.run.assert_not_called()
        self.runner.get.assert_not_called()

    def test_rejects_missing_directory_and_symlink_inputs(self):
        regular = self.fixture('input.png', b'image')
        link = self.root / 'link.png'
        link.symlink_to(regular)
        for source in [self.root / 'absent', self.root, link]:
            with self.subTest(source=source.name):
                self.runner.args.image_fixture = source
                with self.assertRaisesRegex(ValueError, '--image-fixture'):
                    self.runner.prepare_fixtures('image')
        self.runner.run.assert_not_called()

    def test_rejects_symlink_in_private_state(self):
        outside = self.fixture('outside.png', b'original')
        (self.runner.path / 'robot.png').symlink_to(outside)
        with self.assertRaisesRegex(ValueError, 'regular file'):
            self.runner.prepare_fixtures('image')
        self.assertEqual(outside.read_bytes(), b'original')

    def test_completed_create_operations_do_not_require_fixtures(self):
        self.runner.state['operations'] = {'element_create': {}, 'voice_create': {}}
        self.runner.element()
        self.runner.voice()
        self.assertFalse((self.runner.path / 'robot.png').exists())
        self.assertFalse((self.runner.path / 'generic-synthetic-voice.wav').exists())

    def test_cli_default_remains_dry_run(self):
        with patch.object(sys, 'argv', ['live_prod_regression.py']), \
                patch.object(prod, 'ProductionRunner') as constructor, \
                contextlib.redirect_stdout(io.StringIO()) as output:
            prod.main()
        constructor.assert_not_called()
        self.assertIn('Dry-run only', output.getvalue())


if __name__ == '__main__':
    unittest.main()
