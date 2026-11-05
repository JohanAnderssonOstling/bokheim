#!/usr/bin/env python3
"""Dispatch release binaries on the persistent branch that owns their caches."""
import argparse
import json
import re
import subprocess

WORKFLOWS = {'windows': 'desktop-release.yml', 'macos': 'macos-release.yml', 'kobo': 'kobo-release.yml'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('platform', choices=WORKFLOWS)
    parser.add_argument('--source-commit', required=True)
    parser.add_argument('--repository', default='JohanAnderssonOstling/bokheim-release')
    parser.add_argument('--workflow-ref', default='ci/release-builds')
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    if not re.fullmatch('[0-9a-f]{40}', args.source_commit):
        parser.error('--source-commit must be the full, pushed application commit')
    command = ['gh', 'workflow', 'run', WORKFLOWS[args.platform], '--repo', args.repository,
               '--ref', args.workflow_ref, '-f', 'source_commit=' + args.source_commit,
               '-f', 'build_only=true', '-f', 'publish=false']
    if args.dry_run:
        print(json.dumps(command))
    else:
        subprocess.run(command, check=True)


if __name__ == '__main__':
    main()
