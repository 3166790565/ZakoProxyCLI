import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('release_version', ROOT / 'scripts/check-release-version.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ReleaseVersionTests(unittest.TestCase):
    def test_repository_consistent(self):
        self.assertRegex(MODULE.check(ROOT), r'^\d+\.\d+\.\d+$')

    def test_stale_docker_label_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Path(directory)
            for relative in ['Cargo.toml', 'Cargo.lock', 'web/package.json', 'web/package-lock.json', 'Dockerfile']:
                destination = fixture / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / relative, destination)
            dockerfile = fixture / 'Dockerfile'
            text = dockerfile.read_text(encoding='utf-8')
            current = MODULE.check(ROOT)
            dockerfile.write_text(text.replace(f'image.version="{current}"', 'image.version="0.0.0"'), encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'Release version mismatch'):
                MODULE.check(fixture)


if __name__ == '__main__':
    unittest.main()
