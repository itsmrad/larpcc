"""Validate planning fixtures against the deliberately small schema vocabulary.
Import is OUT OF SCOPE. Stdlib only; no general JSON Schema validator claim.
"""
import copy
import ipaddress
import json
import re
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
        assert re.search(rule.get('pattern', ''), value)
    elif kind == 'boolean':
        assert isinstance(value, bool)
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
                           'alias.add', 'alias.remove', 'skills.enable', 'skills.disable'}:
                assert data['profile_id'] is not None and data['name'] is not None
            if command in {'mcp.enable', 'mcp.disable', 'mcp.add', 'mcp.remove',
                           'skills.add', 'skills.remove'}:
                assert data['name'] is not None
            if command in {'profile.migrate', 'provider.bind', 'app.bind',
                           'skills.sync', 'mcp.sync', 'mcp.sync-all'}:
                assert data['profile_id'] is None and data['name'] is None
    elif isinstance(value, list):
        for v in value:
            semantic(v)

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
for alias in ('-oProxyCommand=x', 'alias with space', 'bad*alias'):
    invalid = copy.deepcopy(host_list)
    invalid['data']['hosts'][1]['ssh']['alias'] = alias
    assert not matches(invalid, SCHEMA)
    negative_checks += 1
for command in ('host.catalog.share', 'host.catalog.apply',
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
print(f'{len(fixtures)} valid envelopes; all {len(expected)} success variants; {negative_checks} negative checks passed')
