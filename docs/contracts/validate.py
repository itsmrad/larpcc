"""Validate planning fixtures against the deliberately small schema vocabulary.
Import is OUT OF SCOPE. Stdlib only; no general JSON Schema validator claim.
"""
import copy
import ipaddress
import json
import math
import re
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SCHEMA = json.loads((ROOT / 'cli-v1.schema.json').read_text())

def check(value, rule):
    if '$ref' in rule:
        return check(value, SCHEMA['$defs'][rule['$ref'].split('/')[-1]])
    if 'oneOf' in rule:
        assert sum(matches(value, x) for x in rule['oneOf']) == 1
    if 'anyOf' in rule:
        assert any(matches(value, x) for x in rule['anyOf'])
    if 'const' in rule:
        assert type(value) is type(rule['const']) and value == rule['const']
    if 'enum' in rule:
        assert value in rule['enum']
    kind = rule.get('type')
    if kind == 'object':
        assert isinstance(value, dict)
        assert set(rule['required']) <= value.keys()
        assert value.keys() <= rule['properties'].keys()
        for key, item in value.items():
            check(item, rule['properties'][key])
    elif kind == 'array':
        assert isinstance(value, list)
        assert len(value) <= rule.get('maxItems', len(value))
        for item in value:
            check(item, rule.get('items', {}))
    elif kind == 'string':
        assert isinstance(value, str)
        if 'pattern' in rule:
            assert re.fullmatch(rule['pattern'], value)
    elif kind == 'boolean':
        assert isinstance(value, bool)
    elif kind in ('number', 'integer'):
        assert isinstance(value, (int, float)) and not isinstance(value, bool)
        assert kind == 'number' or isinstance(value, int)
        assert math.isfinite(value)
        assert rule.get('minimum', value) <= value <= rule.get('maximum', value)
    elif kind == 'null':
        assert value is None

def matches(value, rule):
    try:
        check(value, rule)
        return True
    except AssertionError:
        return False

DNS_LABEL = r'[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?'
DESTINATION = re.compile(
    rf'([A-Za-z0-9_][A-Za-z0-9_-]{{0,63}})@({DNS_LABEL}(?:\.{DNS_LABEL})*)'
    r'(?::([1-9][0-9]{0,4}))?'
)

def check_host(host):
    ssh = host['ssh']
    identity_file = host['identity_file']
    if ssh['mode'] == 'config':
        assert identity_file is None
        return
    if identity_file is not None:
        assert '%' not in identity_file and '$' not in identity_file
        assert not any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in identity_file)
        assert (identity_file.startswith('/') and not identity_file.startswith('//')
                or re.match(r'^[A-Za-z]:[/\\]', identity_file))
    match = DESTINATION.fullmatch(ssh['destination'])
    assert match is not None
    user, hostname, port = match.groups()
    assert len(hostname) <= 253
    last = hostname.rsplit('.', 1)[-1]
    if not last[0].isalpha():
        try:
            assert str(ipaddress.IPv4Address(hostname)) == hostname
        except ipaddress.AddressValueError as error:
            raise AssertionError('invalid IPv4 destination') from error
    port = port or '22'
    assert int(port) <= 65535
    assert ssh['resolved'] == f'{user}@{hostname}:{port}'

def semantic(value):
    if isinstance(value, dict):
        for k, v in value.items():
            if k in ('id', 'profile_id') and v is not None:
                assert 0 < int(v) <= 18446744073709551615
            if k == 'revision' and v is not None:
                assert int(v) <= 18446744073709551615
            semantic(v)
        if 'rows' in value and 'profiles' in value:
            assert all(len(row['enabled']) == len(value['profiles']) for row in value['rows'])
        if value.get('command') in ('profile.list', 'alias.list') and value.get('ok'):
            rows = value['data'].get('profiles', value['data'].get('aliases'))
            assert [x['name'] for x in rows] == sorted(set(x['name'] for x in rows))
            if value['command'] == 'profile.list':
                assert len({x['id'] for x in rows}) == len(rows)
        if value.get('ok'):
            command, data = value['command'], value['data']
            if command in ('host.list', 'host.catalog.list'):
                rows = data['hosts'] if command == 'host.list' else data['entries']
                assert [x['name'] for x in rows] == sorted(set(x['name'] for x in rows))
                if command == 'host.list':
                    for host in rows:
                        check_host(host)
            if command == 'host.show':
                check_host(data['host'])
        if value.get('ok') and 'changed' in value.get('data', {}):
            command, data = value['command'], value['data']
            if command in {'profile.add', 'profile.rename', 'profile.forget',
                           'profile.delete', 'profile.recover', 'profile.surface',
                           'alias.add', 'alias.remove', 'skills.enable', 'skills.disable',
                           'profile.plan-label', 'settings.usage.claude-statusline'}:
                assert data['profile_id'] is not None and data['name'] is not None
            if command in {'mcp.enable', 'mcp.disable', 'mcp.add', 'mcp.remove',
                           'skills.add', 'skills.remove'}:
                assert data['name'] is not None
            if command in {'profile.migrate', 'provider.bind', 'app.bind',
                           'skills.sync', 'mcp.sync', 'mcp.sync-all',
                           'settings.usage.live', 'settings.usage.interval'}:
                assert data['profile_id'] is None and data['name'] is None
            if command == 'settings.usage.mode':
                assert (data['profile_id'] is None) == (data['name'] is None)
        if value.get('ok') and value.get('command') == 'usage':
            usage_semantic(value['data'])
        if value.get('ok') and value.get('command') == 'settings.usage':
            settings_semantic(value['data'])
    elif isinstance(value, list):
        for v in value:
            semantic(v)

# ---- Stage 11 usage (docs/stages/11-usage.md). Exact integer pace reference. ----
SKEW_SECONDS = 300
FIVE_HOUR, WEEKLY = 18000, 604800

def epoch(ts):
    return int(datetime.strptime(ts, '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc).timestamp())

def stamp(seconds):
    return datetime.fromtimestamp(seconds, timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')

def tenths(num, den):
    """num/den rounded half away from zero to an integer count of tenths, as a 1-decimal float."""
    q = (2 * abs(num) + den) // (2 * den)
    return (q if num >= 0 else -q) / 10

def pace(captured_at, resets_at, used, window):
    if None in (captured_at, resets_at, used, window):
        return None
    t, r = epoch(captured_at), epoch(resets_at)
    if r <= t or r - t > window + SKEW_SECONDS:
        return None  # reset_since_capture, or clock_skew diagnostic
    u10 = round(used * 10)
    elapsed = min(max(window - (r - t), 0), window)  # E*W in whole seconds
    out = {'elapsed_percent': tenths(1000 * elapsed, window),
           'reserve_percent': tenths(1000 * elapsed - u10 * window, window)}
    if u10 == 0:
        runs, lasts = r, True
    elif elapsed == 0:
        runs, lasts = None, None
    else:
        x = t + (1000 - u10) * elapsed // u10
        runs, lasts = (r, True) if x >= r else (x, False)
    out['runs_out_at'] = None if runs is None else stamp(runs)
    out['lasts_until_reset'] = lasts
    return out

def one_decimal(x):
    return abs(x * 10 - round(x * 10)) < 1e-9

def window_semantic(w, captured_at):
    used, resets = w['used_percent'], w['resets_at']
    assert (w['reason'] is None) == (used is not None and resets is not None)
    assert used is None or one_decimal(used)
    expected = pace(captured_at, resets, used, w['window_seconds']) if w['reason'] is None else None
    assert w['pace'] == expected, (w, expected)

def credits_semantic(c):
    state, count, es = c['state'], c['count'], c['expires_state']
    if state == 'available':
        assert isinstance(count, int) and count >= 1
    elif state == 'none':
        assert count == 0 and es == 'none'
    else:
        assert count is None and es == state
    assert (c['expires_at'] is not None) == (es == 'recorded')

SOURCES = {'claude_statusline': 'claude', 'codex_session_log': 'codex'}

def usage_semantic(data):
    rows = data['profiles']
    assert [p['name'] for p in rows] == sorted({p['name'] for p in rows})
    assert len({p['profile_id'] for p in rows}) == len(rows)
    if data['requested_mode'] != 'settings':
        assert all(p['mode'] == data['requested_mode'] for p in rows)
    for p in rows:
        src, ws = p['source'], p['windows']
        assert ws['five_hour']['window_seconds'] == FIVE_HOUR
        assert ws['weekly']['window_seconds'] == WEEKLY
        if src is None:
            assert p['captured_at'] is None
        else:
            assert p['captured_at'] is not None
            assert SOURCES.get(src, p['provider']) == p['provider']
            assert (src == 'live_endpoint') == (p['mode'] == 'live')
            assert epoch(p['captured_at']) <= epoch(data['generated_at']) + SKEW_SECONDS
        windows = [ws['five_hour'], ws['weekly']] + (ws['extra'] or [])
        if p['captured_at'] is None:
            assert all(w['used_percent'] is None for w in windows)
        for w in windows:
            window_semantic(w, p['captured_at'])
        if ws['extra'] is not None:
            keys = [(w['label'], w['window_seconds'] is None, w['window_seconds'] or 0) for w in ws['extra']]
            assert keys == sorted(keys) and len({w['label'] for w in ws['extra']}) == len(keys)
            assert all(w['pace'] is None for w in ws['extra'] if w['window_seconds'] is None)
        credits_semantic(p['reset_credits'])
        if p['plan'] is not None and p['plan']['source'] == 'client_local':
            assert p['provider'] == 'codex' and src == 'codex_session_log'
        if p['plan'] is not None and p['plan']['source'] == 'live_endpoint':
            assert p['provider'] == 'codex' and src == 'live_endpoint'
        diags = [(d['code'], d['message']) for d in p['diagnostics']]
        assert diags == sorted(diags)

def settings_semantic(data):
    rows = data['profiles']
    assert [p['name'] for p in rows] == sorted({p['name'] for p in rows})
    assert all((p['claude_statusline'] is None) == (p['provider'] == 'codex') for p in rows)
    live = data['live']
    if live['acknowledged_warning'] is not None:
        assert live['acknowledged_warning'] <= live['current_warning']
    if data['mode'] == 'live' or any(p['mode'] == 'live' for p in rows):
        assert live['enabled'] and live['acknowledged_warning'] == live['current_warning']

def schema_walk(rule):
    if isinstance(rule, dict):
        if '$ref' in rule:
            assert rule['$ref'].startswith('#/$defs/')
            assert rule['$ref'].split('/')[-1] in SCHEMA['$defs']
        for v in rule.values():
            schema_walk(v)
    elif isinstance(rule, list):
        for v in rule:
            schema_walk(v)

schema_walk(SCHEMA)
fixtures = json.loads((ROOT / 'cli-v1.examples.json').read_text())
for fixture in fixtures:
    check(fixture, SCHEMA)
    semantic(fixture)
# Validate rejection, not just happy-path fixture acceptance.
for mutate in (
    lambda x: x.update(schema=2),
    lambda x: x.update(unexpected=True),
    lambda x: x.update(ok=False),
    lambda x: x['data']['profiles'][0].update(id='0'),
    lambda x: x['data']['profiles'][0].update(provider='other'),
):
    invalid = copy.deepcopy(fixtures[0])
    mutate(invalid)
    assert not matches(invalid, SCHEMA)
invalid = copy.deepcopy(fixtures[0])
invalid['data']['profiles'][0]['id'] = '18446744073709551616'
try:
    semantic(invalid)
except AssertionError:
    pass
else:
    raise AssertionError('u64 overflow accepted')
commands = {x['command'] for x in fixtures if x['ok']}
expected = {x['properties']['command']['const'] for x in SCHEMA['oneOf'][:-1]}
assert commands == expected, (expected - commands, commands - expected)
negative_checks = 6
invalid = copy.deepcopy(fixtures[0])
duplicate = copy.deepcopy(invalid['data']['profiles'][0])
duplicate['name'] = 'other'
invalid['data']['profiles'].append(duplicate)
invalid_mutations = [invalid]
host_list = next(x for x in fixtures if x['command'] == 'host.list' and x['ok'])
invalid = copy.deepcopy(host_list)
invalid['data']['hosts'].append(copy.deepcopy(invalid['data']['hosts'][0]))
invalid_mutations.append(invalid)
for field, destination in (
    ('destination', '-oProxyCommand=x'),
    ('destination', '-o@host'),
    ('destination', 'dev@-host'),
    ('destination', 'dev@-oProxyCommand=x'),
    ('resolved', '-oProxyCommand=x'),
    ('destination', 'dev@0x7f000001'),
):
    invalid = copy.deepcopy(host_list)
    invalid['data']['hosts'][0]['ssh'][field] = destination
    invalid_mutations.append(invalid)
for identity_file in ('relative', '-i', '/keys/%h', '/keys/$HOME', '/keys/\nfile',
                      r'\\server\share\key', r'\\?\C:\keys\id'):
    invalid = copy.deepcopy(host_list)
    invalid['data']['hosts'][0]['identity_file'] = identity_file
    invalid_mutations.append(invalid)
invalid = copy.deepcopy(host_list)
invalid['data']['hosts'][1]['identity_file'] = '/keys/identity'
invalid_mutations.append(invalid)
for alias in ('-oProxyCommand=x', 'alias with space', 'bad*alias', 'alias\n'):
    invalid = copy.deepcopy(host_list)
    invalid['data']['hosts'][1]['ssh']['alias'] = alias
    assert not matches(invalid, SCHEMA)
    negative_checks += 1
for command in ('host.catalog.list', 'host.catalog.share', 'host.catalog.apply',
                'host.catalog.copy', 'host.catalog.import'):
    invalid = copy.deepcopy(next(x for x in fixtures if x['command'] == command and x['ok']))
    invalid['revision'] = None
    assert not matches(invalid, SCHEMA)
    negative_checks += 1
for command in ('profile.add', 'profile.rename', 'profile.forget',
                'profile.delete', 'profile.recover', 'profile.surface',
                'alias.add', 'alias.remove', 'skills.enable', 'skills.disable'):
    invalid = copy.deepcopy(next(x for x in fixtures if x['command'] == command and x['ok']))
    invalid['data']['profile_id'] = None
    invalid_mutations.append(invalid)
for invalid in invalid_mutations:
    check(invalid, SCHEMA)  # Structurally valid; semantic validation must refuse.
    try:
        semantic(invalid)
    except AssertionError:
        negative_checks += 1
    else:
        raise AssertionError('invalid identity accepted')

# ---- Stage 11 usage: pace vectors, grammar and negative fixtures ----
vectors = json.loads((ROOT / 'usage-pace-vectors.json').read_text())
for v in vectors:
    got = pace(v['captured_at'], v['resets_at'], v['used_percent'], v['window_seconds'])
    assert got == v['expect'], (v['name'], got)
assert len(vectors) >= 15
for label, ok in (('Pro', True), ('Max 5x', True), ('Max 20x', True), ('Team (annual)', True),
                  ('Plus/edu', True), ('x' * 32, True), ('x' * 33, False), ('', False),
                  (' Pro', False), ('Pro ', False), ('Max  5x', False), ('me@example.com', False),
                  ('Pro\n', False), ('\u00dcber', False), ('-Pro', False), ('"Pro"', False)):
    assert matches(label, SCHEMA['$defs']['plan_label']) == ok, label
assert matches('Opus weekly', SCHEMA['$defs']['usage_extra_label'])
assert not matches('Opus weekly ', SCHEMA['$defs']['usage_extra_label'])
usage = next(x for x in fixtures if x['command'] == 'usage' and x['ok'] and x['data']['profiles'][1:])
codex = lambda x: x['data']['profiles'][1]
def set_window(**kw):
    return lambda x: codex(x)['windows']['five_hour'].update(**kw)
def extra(n):
    def mutate(x):
        base = copy.deepcopy(codex(x)['windows']['five_hour'])
        codex(x)['windows']['extra'] = [dict(base, label=f'Bucket {i:02d} 5h') for i in range(n)]
    return mutate
structural = (
    set_window(reason='unknown_reason'),
    set_window(used_percent=100.5),
    set_window(used_percent=-1),
    set_window(used_percent=True),
    set_window(window_seconds=1.5),
    lambda x: codex(x).update(captured_at='2026-11-21T12:00:00.000Z'),
    lambda x: codex(x).update(captured_at='2026-11-21T12:00:00+00:00'),
    lambda x: codex(x).update(plan={'label': 'me@example.com', 'source': 'user'}),
    lambda x: codex(x).update(plan={'label': 'Pro', 'source': 'client'}),
    lambda x: codex(x).update(source='other'),
    lambda x: codex(x).update(access_token='fake-token-0000'),
    lambda x: codex(x).update(email='user@example.invalid'),
    lambda x: codex(x).update(account_id='fake-account'),
    lambda x: codex(x).update(cost_usd=1.0),
    lambda x: codex(x)['windows'].update(input_tokens=1),
    lambda x: codex(x)['reset_credits'].pop('expires_at'),
    lambda x: codex(x)['reset_credits'].update(state='unknown'),
    extra(9),
)
for mutate in structural:
    invalid = copy.deepcopy(usage)
    mutate(invalid)
    assert not matches(invalid, SCHEMA), mutate
    negative_checks += 1
def two_extras_reversed(x):
    extra(2)(x)
    codex(x)['windows']['extra'].reverse()
def duplicate_extras(x):
    extra(2)(x)
    codex(x)['windows']['extra'][1]['label'] = codex(x)['windows']['extra'][0]['label']
live_usage = next(x for x in fixtures if x['command'] == 'usage' and x['ok']
                  and x['data']['requested_mode'] == 'live')
semantic_only = [
    (usage, set_window(reason=None, used_percent=None)),
    (usage, set_window(reason='not_reported')),
    (usage, set_window(used_percent=42.25)),
    (usage, set_window(window_seconds=604800)),
    (usage, lambda x: codex(x)['windows']['five_hour']['pace'].update(reserve_percent=30.1)),
    (usage, lambda x: codex(x)['windows']['five_hour']['pace'].update(lasts_until_reset=False)),
    (usage, lambda x: codex(x)['reset_credits'].update(state='none', count=3, expires_state='none')),
    (usage, lambda x: codex(x)['reset_credits'].update(state='unsupported', count=2, expires_state='unsupported')),
    (usage, lambda x: codex(x)['reset_credits'].update(state='available', count=0)),
    (usage, lambda x: codex(x)['reset_credits'].update(expires_state='recorded')),
    (usage, two_extras_reversed),
    (usage, duplicate_extras),
    (usage, lambda x: x['data']['profiles'].reverse()),
    (usage, lambda x: codex(x).update(source='claude_statusline')),
    (usage, lambda x: codex(x).update(mode='live')),
    (usage, lambda x: codex(x).update(source=None)),
    (usage, lambda x: codex(x).update(captured_at='2026-11-21T13:00:00Z')),
    (usage, lambda x: x['data'].update(requested_mode='live')),
    (usage, lambda x: x['data']['profiles'][0].update(plan={'label': 'max', 'source': 'client_local'})),
    (usage, lambda x: codex(x).update(plan={'label': 'pro', 'source': 'live_endpoint'})),
    (live_usage, lambda x: x['data']['profiles'][0].update(plan={'label': 'max', 'source': 'live_endpoint'})),
    (live_usage, lambda x: x['data']['profiles'][1].update(plan={'label': 'pro', 'source': 'client_local'})),
    (next(x for x in fixtures if x['command'] == 'settings.usage'),
     lambda x: x['data']['profiles'][1].update(claude_statusline=True)),
    (next(x for x in fixtures if x['command'] == 'settings.usage.claude-statusline' and x['ok']),
     lambda x: x['data'].update(profile_id=None, name=None)),
    (next(x for x in fixtures if x['command'] == 'settings.usage'), lambda x: x['data'].update(mode='live')),
    (next(x for x in fixtures if x['command'] == 'profile.plan-label'),
     lambda x: x['data'].update(profile_id=None)),
    (next(x for x in fixtures if x['command'] == 'settings.usage.live'),
     lambda x: x['data'].update(profile_id='2')),
]
for base, mutate in semantic_only:
    invalid = copy.deepcopy(base)
    mutate(invalid)
    check(invalid, SCHEMA)  # Structurally valid; semantic validation must refuse.
    try:
        semantic(invalid)
    except AssertionError:
        negative_checks += 1
    else:
        raise AssertionError(f'invalid usage fixture accepted: {mutate}')
print(f'{len(fixtures)} valid envelopes; all {len(expected)} success variants; {len(vectors)} pace vectors; {negative_checks} negative checks passed')
