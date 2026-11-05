"""Release inputs shared by local builds, CI and exact-commit verification."""
import argparse
import json
import os
from pathlib import Path
import re
from urllib.parse import urlsplit

CONFIG = Path(__file__).with_name('release-config.json')
DEPENDENCIES = {'GPUI-Fork': 'GPUI_FORK_REF', 'HtmlEngine': 'HTML_ENGINE_REF', 'HtmlViewCore': 'HTML_VIEW_CORE_REF'}
UPDATE_ENVIRONMENT = {'public_key_hex': 'BOKHEIM_UPDATE_PUBLIC_KEY_HEX', 'key_id': 'BOKHEIM_UPDATE_KEY_ID', 'endpoint': 'BOKHEIM_UPDATE_ENDPOINT'}


def load():
    config = json.loads(CONFIG.read_text())
    if set(config['dependencies']) != set(DEPENDENCIES):
        raise ValueError('Release configuration must pin each client dependency')
    for name, revision in config['dependencies'].items():
        if not isinstance(revision, str) or not re.fullmatch('[0-9a-f]{40}', revision):
            raise ValueError(f'Invalid release dependency commit for {name}')
    if not re.fullmatch('[A-Za-z0-9_.+-]+', config['rust_version']):
        raise ValueError('Invalid release Rust toolchain')
    return config


def environment(overrides):
    config = load()
    values = {'RUST_VERSION': config['rust_version'], 'RUSTUP_TOOLCHAIN': config['rust_version']}
    values.update({variable: config['dependencies'][name] for name, variable in DEPENDENCIES.items()})
    for key, variable in UPDATE_ENVIRONMENT.items():
        values[variable] = overrides.get(variable) or config['update'][key]
    if not re.fullmatch('[0-9a-fA-F]{64}', values['BOKHEIM_UPDATE_PUBLIC_KEY_HEX']):
        raise ValueError('Release update public key must contain 32 hexadecimal bytes')
    key_id = values['BOKHEIM_UPDATE_KEY_ID']
    if not key_id or not key_id.isascii() or not key_id.isprintable():
        raise ValueError('Release update key ID must be a nonempty printable identifier')
    endpoint = values['BOKHEIM_UPDATE_ENDPOINT']
    url = urlsplit(endpoint)
    if url.scheme != 'https' or not url.hostname or url.username is not None or url.password is not None or url.fragment or any(c.isspace() for c in endpoint):
        raise ValueError('Release update endpoint must use HTTPS without credentials or fragments')
    return values


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--github-env', action='store_true', help='Append validated public build inputs to GITHUB_ENV')
    args = parser.parse_args()
    values = environment(os.environ)
    if args.github_env:
        with Path(os.environ['GITHUB_ENV']).open('a', encoding='utf-8', newline='\n') as output:
            output.write(''.join(f'{key}={value}\n' for key, value in values.items()))
    else:
        print(json.dumps(values, indent=2))


if __name__ == '__main__':
    main()
