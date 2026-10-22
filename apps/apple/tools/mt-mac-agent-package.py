#!/usr/bin/env python3
"""Package the native service before Xcode signs the enclosing app."""
import os
import pathlib
import plistlib


def service(bundle_id, executable):
    return {
        'Label': bundle_id + '.VPNRecovery',
        'BundleProgram': 'Contents/MacOS/' + executable,
        'ProgramArguments': [executable, '--vpn-supervisor'],
        'RunAtLoad': True,
        'KeepAlive': True,
        'ThrottleInterval': 45,
        'ProcessType': 'Background',
    }


if __name__ == '__main__':
    # An iOS app running on Mac has no ServiceManagement API. Only Catalyst
    # receives this service, with the same signed executable and app identity.
    if os.environ.get('EFFECTIVE_PLATFORM_NAME') == '-maccatalyst':
        contents = pathlib.Path(os.environ['TARGET_BUILD_DIR']) / os.environ['CONTENTS_FOLDER_PATH']
        folder = contents / 'Library/LaunchAgents'
        folder.mkdir(parents=True, exist_ok=True)
        bundle_id = os.environ['PRODUCT_BUNDLE_IDENTIFIER']
        (folder / (bundle_id + '.VPNRecovery.plist')).write_bytes(
            plistlib.dumps(service(bundle_id, os.environ['EXECUTABLE_NAME'])))
