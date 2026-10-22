"""Exercise the real startup script with local process stubs and temporary state."""
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest

ENTRYPOINT = Path(__file__).with_name("node-entrypoint.sh")
WORDS = "test-recovery-words-never-in-logs"


class Startup(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="montana-entrypoint-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.data = self.root / "data"
        self.manifest = self.root / "manifest.json"
        self.calls = self.root / "calls.jsonl"
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("MONTANA_")}
        self.env.update(
            PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
            MONTANA_DATA_DIR=str(self.data),
            MONTANA_GENESIS_MANIFEST=str(self.manifest),
            TEST_CALLS=str(self.calls),
            TEST_WORDS=WORDS,
        )
        self.stub("chown", "#!/bin/sh\nexit 0\n")
        self.stub("setsid", '#!/bin/sh\n[ "$1" = --wait ] || exit 92\nshift\nexec "$@"\n')
        self.stub("curl", "#!/bin/sh\nexit 93\n")
        self.stub("sha256sum", """#!/usr/bin/env python3
import hashlib, sys
print(hashlib.sha256(open(sys.argv[1], 'rb').read()).hexdigest() + '  ' + sys.argv[1])
""")
        self.stub("runuser", """#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
assert args[:4] == ['-u', 'montana', '--', '/usr/local/bin/montana-node']
args = args[4:]
with open(os.environ['TEST_CALLS'], 'a') as stream:
    stream.write(json.dumps({'argv': args, 'mnemonic_in_environment': 'MONTANA_MNEMONIC' in os.environ}) + '\\n')
if args[0] == 'init':
    assert '--mnemonic' not in args
    data = pathlib.Path(args[args.index('--data-dir') + 1])
    phrase = sys.stdin.read().strip() if '--mnemonic-stdin' in args else os.environ['TEST_WORDS']
    (data / 'identity.bin').write_bytes(b'test identity')
    print(phrase)
elif args[0] == 'start':
    print('node start')
else:
    raise SystemExit(94)
""")

    def stub(self, name, content):
        path = self.bin / name
        path.write_text(content)
        path.chmod(0o700)

    def run_entrypoint(self, **settings):
        return subprocess.run(
            ["bash", str(ENTRYPOINT)], env=self.env | settings,
            capture_output=True, text=True, timeout=5,
        )

    def recorded(self):
        return [json.loads(line) for line in self.calls.read_text().splitlines()]

    def test_missing_manifest_refuses_before_any_state_or_identity(self):
        result = self.run_entrypoint()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.data.exists())
        self.assertFalse(self.calls.exists())

    def test_isolated_mode_is_explicit_and_keeps_recovery_words_private(self):
        result = self.run_entrypoint(MONTANA_MODE="isolated", MONTANA_MNEMONIC=WORDS)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn(WORDS, result.stdout + result.stderr)
        self.assertEqual(self.data.joinpath("mnemonic.txt").read_text().strip(), WORDS)
        self.assertEqual(stat.S_IMODE(self.data.joinpath("mnemonic.txt").stat().st_mode), 0o400)
        calls = self.recorded()
        self.assertEqual([call['argv'][0] for call in calls], ['init', 'start'])
        self.assertTrue(all(not call['mnemonic_in_environment'] for call in calls))
        self.assertNotIn(WORDS, self.calls.read_text())
        self.assertNotIn('--listen', calls[-1]['argv'])

    def test_a_cohort_manifest_selects_only_network_start(self):
        self.manifest.write_text('{"network_name":"test","peers":[]}')
        result = self.run_entrypoint()
        self.assertEqual(result.returncode, 0, result.stderr)
        args = self.recorded()[-1]['argv']
        self.assertIn('--listen', args)
        self.assertEqual(args[args.index('--genesis-manifest') + 1], str(self.manifest))

    def test_manifest_pin_and_isolated_conflict_refuse_before_init(self):
        self.manifest.write_text('{}')
        for settings in [dict(MONTANA_MANIFEST_SHA256="0" * 64), dict(MONTANA_MODE="isolated")]:
            with self.subTest(settings=settings):
                self.assertNotEqual(self.run_entrypoint(**settings).returncode, 0)
                self.assertFalse(self.data.exists())
                self.assertFalse(self.calls.exists())

    def test_existing_recovery_file_is_preserved(self):
        self.data.mkdir()
        recovery = self.data / 'mnemonic.txt'
        recovery.write_text('existing recovery')
        result = self.run_entrypoint(MONTANA_MODE="isolated")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(recovery.read_text(), 'existing recovery')
        self.assertFalse(self.calls.exists())

    def test_mounted_secret_is_read_from_stdin(self):
        secret = self.root / 'recovery'
        secret.write_text(WORDS)
        result = self.run_entrypoint(MONTANA_MODE="isolated", MONTANA_MNEMONIC_FILE=str(secret))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn(WORDS, result.stdout + result.stderr + self.calls.read_text())
        self.assertEqual(self.data.joinpath('mnemonic.txt').read_text().strip(), WORDS)


if __name__ == '__main__':
    unittest.main()
