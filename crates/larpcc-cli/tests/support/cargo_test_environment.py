"""Refuse Cargo acceptance runs outside the private test environment."""
import os
import sys
from pathlib import Path


def require_private_test_environment(script_name):
    test_parent = os.environ.get('LARPCC_TEST_PARENT', '')
    is_ci = os.environ.get('GITHUB_ACTIONS') == 'true'
    vendor_vars = ('CODEX_HOME', 'CLAUDE_CONFIG_DIR', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME',
                   'ANTHROPIC_API_KEY', 'OPENAI_API_KEY')
    wrapped = (Path(os.environ.get('HOME', '')).name.startswith('.larpcc-cargo-home.')
               and all(name not in os.environ for name in vendor_vars))
    if not os.path.isdir(test_parent) or not (is_ci or wrapped):
        print(f'{script_name} requires an existing LARPCC_TEST_PARENT and GitHub Actions or larpcc-cargo.',
              file=sys.stderr)
        sys.exit(2)
