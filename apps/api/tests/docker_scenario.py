"""Run one stage of the Docker restart scenario inside the isolated test network."""

import json
import sys
import time

import httpx
from sqlalchemy import func, select
from test_integration import wait_simulator

from app.db.session import SessionLocal
from app.modules.models import Base, Loan, ToolPlacement

API = 'http://api:8000'


def request(method, path, **kwargs):
    response = httpx.request(method, API + path, timeout=3, **kwargs)
    response.raise_for_status()
    return response.json()


def wait_ready():
    deadline = time.monotonic() + 45
    last_error = 'no response'
    while time.monotonic() < deadline:
        try:
            response = httpx.get(API + '/health/ready', timeout=2)
            if response.status_code == 200:
                wait_simulator()
                return
            last_error = f'{response.status_code}: {response.text}'
        except httpx.HTTPError as error:
            last_error = str(error)
        time.sleep(0.2)
    raise RuntimeError(f'API MQTT SUBACK/readiness timeout: {last_error}')


def operation(path):
    pending = request('POST', path)
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        item = next(x for x in request('GET', '/api/v1/operations') if x['id'] == pending['id'])
        if item['status'] != 'PENDING':
            assert item['status'] == 'CONFIRMED', item
            return item
        time.sleep(0.2)
    raise RuntimeError(f'MQTT operation confirmation timeout: {pending["id"]}')


def snapshot():
    with SessionLocal() as session:
        counts = {
            table.name: session.scalar(select(func.count()).select_from(table))
            for table in Base.metadata.sorted_tables
        }
        loans = sorted(repr(tuple(row)) for row in session.execute(select(Loan.__table__)))
        places = sorted(
            repr(tuple(row)) for row in session.execute(select(ToolPlacement.__table__))
        )
    return {'counts': counts, 'loans': loans, 'placements': places}


def checkout(tool, user, branch):
    auth = request('POST', '/api/v1/authorizations', json={
        'tool_id': tool, 'user_id': user, 'branch_id': branch,
    })
    request('POST', f'/api/v1/authorizations/{auth["id"]}/approve')
    return operation(f'/api/v1/authorizations/{auth["id"]}/checkout')['loan_id']


def main():
    wait_ready()
    if sys.argv[1] == 'empty':
        assert request('GET', '/api/v1/users') == []
        assert request('GET', '/api/v1/tools') == []
        print('initial database is empty: no automatic seed')
    elif sys.argv[1] == 'before':
        request('POST', '/api/v1/auth/login', json={'username': 'admin', 'password': 'admin'})
        tool = request('GET', '/api/v1/tools')[0]['id']
        user = request('GET', '/api/v1/users')[0]['id']
        branch = request('GET', '/api/v1/branches')[0]['id']
        loan = checkout(tool, user, branch)
        print(json.dumps({'snapshot': snapshot(), 'tool': tool, 'user': user,
                          'branch': branch, 'loan': loan}))
    elif sys.argv[1] == 'after':
        before = json.load(sys.stdin)
        assert snapshot() == before['snapshot'], 'database changed during API container restart'
        operation(f'/api/v1/loans/{before["loan"]}/return')
        loan = checkout(before['tool'], before['user'], before['branch'])
        operation(f'/api/v1/loans/{loan}/return')
        returned = next(x for x in request('GET', '/api/v1/loans') if x['id'] == loan)
        assert returned['status'] == 'RETURNED'
        print('exact restart snapshot matched; return, checkout and return succeeded')
    else:
        raise ValueError('unknown scenario stage')


if __name__ == '__main__':
    main()
