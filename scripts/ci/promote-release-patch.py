#!/usr/bin/env python3
"""Replace release downloads with an explicitly selected, fully staged patch."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile

import release_artifacts as artifacts


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository', required=True)
    parser.add_argument('--staging-tag', required=True)
    parser.add_argument('--target-tag', required=True)
    parser.add_argument('--source-commit', required=True)
    parser.add_argument('--kobo-source-commit', required=True)
    parser.add_argument('--previous-source-commit', required=True)
    args = parser.parse_args()
    assert re.fullmatch(r'[\w.-]+/[\w.-]+', args.repository)
    match = re.fullmatch(r'v([0-9]+\.[0-9]+\.[0-9]+)', args.target_tag)
    assert match and re.fullmatch(re.escape(args.target_tag) + r'-patch[1-9][0-9]*', args.staging_tag)
    version = match.group(1)
    for sha in (args.source_commit, args.kobo_source_commit, args.previous_source_commit):
        assert re.fullmatch('[0-9a-f]{40}', sha)
    def release(tag):
        return json.loads(command('gh', 'release', 'view', tag, '--repo', args.repository,
                                  '--json', 'isDraft,assets,body'))
    staging, target = release(args.staging_tag), release(args.target_tag)
    assert staging['isDraft'] and not target['isDraft'], 'Expected staged patch and published target'
    with tempfile.TemporaryDirectory() as scratch:
        directory = Path(scratch)
        command('gh', 'release', 'download', args.staging_tag, '--repo', args.repository, '--dir', scratch)
        assert (directory / 'desktop-source-commit.txt').read_text().strip() == args.source_commit
        assert (directory / 'kobo-source-commit.txt').read_text().strip() == args.kobo_source_commit
        expected = {item['name']: item for item in staging['assets']}
        for name, asset in expected.items():
            actual = artifacts.fingerprint(directory / name)
            assert asset['size'] == actual['bytes'] and asset['digest'] == 'sha256:' + actual['sha256'], name
        compatibility = None
        for platform in ('android', 'linux', 'windows'):
            receipt = json.loads((directory / artifacts.receipt_name(platform)).read_text())
            artifacts.validate(receipt, platform, args.source_commit, args.repository, directory)
            info = json.loads((directory / ('update-info-' + artifacts.TARGETS[platform] + '.json')).read_text())
            assert info['application'] == version and info['target'] == artifacts.TARGETS[platform]
            current = (info['schema'], info['taxonomy_formats'], info['update_trust'])
            assert compatibility is None or compatibility == current, 'Platform compatibility differs'
            compatibility = current
            if platform == 'windows':
                run = json.loads(command('gh', 'api', f'repos/{args.repository}/actions/runs/{receipt["workflow_run"]}'))
                assert run['head_sha'] == args.source_commit and run['conclusion'] == 'success'
            elif platform == 'android':
                assert info['apk_sha256'] == artifacts.fingerprint(directory / 'Bokheim-Android-arm64.apk')['sha256']
                assert info['android']['debuggable'] is False
        mac = json.loads((directory / 'build-macos.json').read_text())
        assert mac['source_commit'] == args.source_commit and mac['application'] == version
        assert set(mac['architectures']) == {'arm64', 'x86_64'}
        image = directory / 'Bokheim-macOS-universal.dmg'
        assert mac['artifact']['name'] == image.name
        assert artifacts.fingerprint(image) == {key: mac['artifact'][key] for key in ('bytes', 'sha256')}
        for name in ('Bokheim-Kobo-Installer-Windows-x86_64.exe', 'Bokheim-Kobo-Installer-Linux-x86_64.AppImage',
                     'Bokheim-Kobo-armv7.tar.gz', 'Bokheim-Kobo-Libra2-armv7.tar.gz'):
            assert (directory / (name + '.sha256')).read_text().split() == [artifacts.fingerprint(directory / name)['sha256'], name]
        assert artifacts.fingerprint(directory / 'Bokheim-Kobo-armv7.tar.gz') == artifacts.fingerprint(directory / 'Bokheim-Kobo-Libra2-armv7.tar.gz'), 'Legacy Kobo alias differs from the universal package'
        provenance = json.loads((directory / 'release-provenance.json').read_text())
        assert provenance['source_commit'] == args.source_commit and provenance['version'] == version
        previous = directory / 'previous'
        previous.mkdir()
        command('gh', 'release', 'download', args.target_tag, '--repo', args.repository,
                '--pattern', 'desktop-source-commit.txt', '--dir', str(previous))
        assert (previous / 'desktop-source-commit.txt').read_text().strip() in (args.previous_source_commit, args.source_commit), 'Published source has changed'
        (directory / 'previous-release.json').write_text(json.dumps(target, indent=2) + '\n')
        # Publish the generation marker after every package has been replaced.
        names = sorted(name for name in expected if name != 'desktop-source-commit.txt')
        names.append('desktop-source-commit.txt')
        for name in names:
            command('gh', 'release', 'upload', args.target_tag, '--repo', args.repository,
                    '--clobber', str(directory / name))
        command('gh', 'release', 'edit', args.target_tag, '--repo', args.repository, '--notes', staging['body'])
        actual = {asset['name']: asset for asset in release(args.target_tag)['assets']}
        for name in names:
            assert actual[name]['digest'] == expected[name]['digest'] and actual[name]['size'] == expected[name]['size'], name
        Path('promoted-release-assets.json').write_text(json.dumps(actual, indent=2) + '\n')
        Path('previous-release.json').write_text(json.dumps(target, indent=2) + '\n')
        print(f'Replaced {args.target_tag} downloads with patch source {args.source_commit}')


if __name__ == '__main__':
    main()
