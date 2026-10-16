import json
from pathlib import Path
import re
import tomllib


def check(root):
    with (root / 'Cargo.toml').open('rb') as source:
        package = tomllib.load(source)['package']
    with (root / 'Cargo.lock').open('rb') as source:
        locked = tomllib.load(source)['package']
    frontend = json.loads((root / 'web/package.json').read_text(encoding='utf-8'))
    npm_lock = json.loads((root / 'web/package-lock.json').read_text(encoding='utf-8'))
    labels = re.findall(
        r'^LABEL org\.opencontainers\.image\.version="([^"]+)"\s*$',
        (root / 'Dockerfile').read_text(encoding='utf-8'), re.MULTILINE,
    )
    versions = {
        'Cargo.toml': package['version'],
        'Cargo.lock': next(item['version'] for item in locked if item['name'] == package['name']),
        'web/package.json': frontend['version'],
        'web/package-lock.json': npm_lock['version'],
        'web/package-lock.json root': npm_lock['packages']['']['version'],
        'Dockerfile OCI version label': labels[0] if len(labels) == 1 else None,
    }
    if any(value != package['version'] for value in versions.values()):
        raise ValueError(f'Release version mismatch: {versions}')
    return package['version']


if __name__ == '__main__':
    print(f'PASS release version consistency: {check(Path(__file__).resolve().parents[1])}')
