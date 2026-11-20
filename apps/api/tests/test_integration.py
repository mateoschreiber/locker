import json
import os
import socket
import subprocess
import sys
import time
from contextlib import contextmanager
from datetime import UTC, datetime
from pathlib import Path
from threading import Event

import httpx
import paho.mqtt.client as mqtt
import pytest
from fastapi.testclient import TestClient
from sqlalchemy import select

pytestmark = pytest.mark.skipif(
    os.getenv("LOCKER_ISOLATED_TEST") != "1", reason="requires isolated PostgreSQL and MQTT"
)


def wait_simulator():
    ready = Event()
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2)

    def on_connect(client, userdata, flags, reason, properties):
        if not reason.is_failure:
            client.subscribe('locker/LAB-LOCKER-001/status', qos=1)

    def on_message(client, userdata, message):
        payload = json.loads(message.payload)
        if payload.get('type') in ('LOCKER_ONLINE', 'LOCKER_HEARTBEAT'):
            timestamp = datetime.fromisoformat(payload['timestamp'])
            age = (datetime.now(UTC) - timestamp).total_seconds()
            if -5 <= age <= 60:
                ready.set()

    client.on_connect = on_connect
    client.on_message = on_message
    client.connect('mosquitto', 1883, keepalive=30)
    client.loop_start()
    try:
        assert ready.wait(15), 'timeout: simulator did not confirm MQTT command subscription'
    finally:
        client.disconnect()
        client.loop_stop()


@contextmanager
def api_process():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    url = f'http://127.0.0.1:{port}'
    process = subprocess.Popen(
        [sys.executable, '-m', 'uvicorn', 'app.main:app', '--host', '127.0.0.1',
         '--port', str(port)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    try:
        for _ in range(100):
            if process.poll() is not None:
                pytest.fail(f'API process exited with code {process.returncode}')
            try:
                if httpx.get(f'{url}/health/live', timeout=1).status_code == 200:
                    break
            except httpx.ConnectError:
                pass
            time.sleep(0.1)
        else:
            pytest.fail('API process did not start')
        yield url
    finally:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def test_simulated_loan_and_restart() -> None:
    from app.db.session import SessionLocal
    from app.main import app
    from app.modules.models import Base, Loan, ToolPlacement
    from app.modules.mqtt import mqtt_bridge
    from app.modules.seed import seed_lab

    assert os.environ['DATABASE_URL'].endswith('@postgres:5432/locker_test')
    assert os.environ['MQTT_HOST'] == 'mosquitto'
    wait_simulator()
    with api_process() as url:
        assert httpx.get(f'{url}/api/v1/users').json() == []
        assert httpx.get(f'{url}/api/v1/tools').json() == []
    with SessionLocal() as session:
        seed_lab(session)
    with api_process() as url:
        response = httpx.post(f'{url}/api/v1/auth/login', json={
            'username': 'admin', 'password': 'admin',
        })
        assert response.status_code == 200

    def wait_operation(client: TestClient, operation_id: str) -> dict:
        for _ in range(100):
            operation = next(item for item in client.get('/api/v1/operations').json()
                             if item['id'] == operation_id)
            if operation['status'] != 'PENDING':
                return operation
            time.sleep(0.1)
        pytest.fail('simulator did not confirm operation')

    with TestClient(app) as client:
        assert mqtt_bridge.subscribed.wait(15), 'timeout: API MQTT SUBACK missing'
        tool = client.get('/api/v1/tools').json()[0]
        user = client.get('/api/v1/users').json()[0]
        branch = client.get('/api/v1/branches').json()[0]
        response = client.post('/api/v1/authorizations', json={
            'tool_id': tool['id'], 'user_id': user['id'], 'branch_id': branch['id']})
        assert response.status_code == 201
        authorization = response.json()
        response = client.post(f"/api/v1/authorizations/{authorization['id']}/approve")
        assert response.status_code == 200
        response = client.post(f"/api/v1/authorizations/{authorization['id']}/checkout")
        assert response.status_code == 202
        operation = wait_operation(client, response.json()['id'])
        assert operation['status'] == 'CONFIRMED'
        loan = next(item for item in client.get('/api/v1/loans').json()
                    if item['id'] == operation['loan_id'])
        assert loan['status'] == 'ACTIVE'
        with SessionLocal() as session:
            placement = session.scalar(
                select(ToolPlacement).where(ToolPlacement.tool_id == tool['id'])
            )
            assert placement is not None and placement.removed_at is not None

    with SessionLocal() as session:
        before_loans = [(x.id, x.status) for x in session.scalars(select(Loan)).all()]
        before_places = [(x.id, x.removed_at) for x in session.scalars(select(ToolPlacement)).all()]
        before = {
            table.name: sorted(repr(row) for row in session.execute(select(table)).all())
            for table in Base.metadata.sorted_tables
        }
    with api_process() as url:
        assert httpx.get(f'{url}/api/v1/loans').status_code == 200
    with SessionLocal() as session:
        after = {
            table.name: sorted(repr(row) for row in session.execute(select(table)).all())
            for table in Base.metadata.sorted_tables
        }
    assert after == before
    with TestClient(app) as client:
        with SessionLocal() as session:
            assert before_loans == [(x.id, x.status) for x in session.scalars(select(Loan)).all()]
            places = session.scalars(select(ToolPlacement)).all()
            assert before_places == [(x.id, x.removed_at) for x in places]
        assert mqtt_bridge.subscribed.wait(15), 'timeout after restart: API MQTT SUBACK missing'
        wait_simulator()
        response = client.post(f"/api/v1/loans/{loan['id']}/return")
        assert response.status_code == 202
        assert wait_operation(client, response.json()['id'])['status'] == 'CONFIRMED'
        assert next(item for item in client.get('/api/v1/loans').json()
                    if item['id'] == loan['id'])['status'] == 'RETURNED'
        with SessionLocal() as session:
            active = session.scalar(select(ToolPlacement).where(
                ToolPlacement.tool_id == tool['id'], ToolPlacement.removed_at.is_(None)))
            assert active is not None
    with SessionLocal() as session, pytest.raises(RuntimeError, match='vacía'):
        seed_lab(session)

    with SessionLocal() as session:
        connection = session.connection()
        statements = Path('integrity.sql').read_text(encoding='utf-8').split(';')
        connection.exec_driver_sql('SET TRANSACTION READ ONLY')
        findings = dict(connection.exec_driver_sql(statements[1]).all())
        assert all(count == 0 for count in findings.values())


def test_status_contract_password_exclusion_and_guards() -> None:
    from uuid import uuid4

    from app.db.session import SessionLocal
    from app.main import app
    from app.modules.models import Loan, LockerOperation

    with TestClient(app) as client:
        # #2: no user payload ever exposes the password column.
        response = client.post('/api/v1/auth/login', json={
            'username': 'admin', 'password': 'admin'})
        assert response.status_code == 200
        assert 'password' not in response.json()['user']
        users = client.get('/api/v1/users').json()
        assert users and all('password' not in user for user in users)
        admin = users[0]
        assert 'password' not in client.get(f"/api/v1/users/{admin['id']}").json()
        created = client.post('/api/v1/users', json={
            'username': f'auditor-{uuid4().hex[:8]}', 'display_name': 'Auditor'})
        assert created.status_code == 201
        assert 'password' not in created.json()
        patched = client.patch(
            f"/api/v1/users/{created.json()['id']}", json={'status': 'INACTIVE'})
        assert patched.status_code == 200
        assert 'password' not in patched.json()

        # #19: unknown states are rejected and leave the row untouched.
        branch = client.get('/api/v1/branches').json()[0]
        rejected = client.patch(f"/api/v1/branches/{branch['id']}", json={'status': 'BANANA'})
        assert rejected.status_code == 422
        assert rejected.json()['detail'] == "Campo 'status': debe ser uno de ACTIVE, INACTIVE"
        assert client.get(f"/api/v1/branches/{branch['id']}").json()['status'] == branch['status']

        locker = client.get('/api/v1/lockers').json()[0]
        rejected = client.patch(f"/api/v1/lockers/{locker['id']}", json={'status': 'ACTIVE'})
        assert rejected.status_code == 422
        assert rejected.json()['detail'] == "Campo 'status': debe ser uno de ONLINE, INACTIVE"

        # Rust parity: null counts as an absent status (PATCH leaves the row
        # untouched; creation applies the default), and non-strings are the
        # same 422 with the same message as Rust's type validation.
        patched = client.patch(f"/api/v1/branches/{branch['id']}", json={'status': None})
        assert patched.status_code == 200
        assert patched.json()['status'] == branch['status']
        created = client.post('/api/v1/tools', json={
            'asset_code': f'TOOL-{uuid4().hex[:8]}', 'name': 'Con nulo', 'status': None})
        assert created.status_code == 201
        assert created.json()['status'] == 'AVAILABLE'
        for invalid in (5, True, ['ACTIVE'], {'code': 'X'}, ''):
            rejected = client.patch(f"/api/v1/branches/{branch['id']}",
                                    json={'status': invalid})
            if isinstance(invalid, str):
                assert rejected.json()['detail'] == (
                    "Campo 'status': debe ser uno de ACTIVE, INACTIVE")
            else:
                assert rejected.json()['detail'] == "Campo 'status': se requiere texto"
            assert rejected.status_code == 422
        assert client.get(f"/api/v1/branches/{branch['id']}").json()['status'] == branch['status']

        # ON_LOAN is domain-only: never administratively creatable.
        rejected = client.post('/api/v1/tools', json={
            'asset_code': f'TOOL-{uuid4().hex[:8]}', 'name': 'X', 'status': 'ON_LOAN'})
        assert rejected.status_code == 422

        # Guards: a tool with an in-flight loan keeps its state.
        tool = next(t for t in client.get('/api/v1/tools').json()
                    if t['status'] == 'AVAILABLE')
        with SessionLocal() as session:
            session.add(Loan(tool_id=tool['id'], user_id=admin['id'], status='CHECKOUT_PENDING'))
            session.commit()
        conflict = client.patch(f"/api/v1/tools/{tool['id']}", json={'status': 'INACTIVE'})
        assert conflict.status_code == 409
        assert conflict.json()['detail'] == 'La herramienta tiene un préstamo activo o pendiente'
        assert client.post(f"/api/v1/tools/{tool['id']}/deactivate").status_code == 409
        assert client.get(f"/api/v1/tools/{tool['id']}").json()['status'] == 'AVAILABLE'
        # Only status is guarded: other fields stay editable.
        renamed = client.patch(f"/api/v1/tools/{tool['id']}", json={'name': 'Renombrada'})
        assert renamed.status_code == 200

        # A compartment holding a placement refuses manual deactivation.
        compartment = client.get('/api/v1/compartments').json()[0]
        conflict = client.post(f"/api/v1/compartments/{compartment['id']}/deactivate")
        assert conflict.status_code == 409
        assert conflict.json()['detail'] == (
            'El compartimiento tiene una ubicación activa o una devolución pendiente')

        # A free compartment reserved by a pending RETURN refuses too.
        free = client.post('/api/v1/compartments', json={
            'locker_id': locker['id'], 'code': f'C-{uuid4().hex[:6]}',
            'name': 'Reservada', 'position': 999})
        assert free.status_code == 201
        free = free.json()
        with SessionLocal() as session:
            loan = Loan(tool_id=tool['id'], user_id=admin['id'], status='ACTIVE')
            session.add(loan)
            session.flush()
            session.add(LockerOperation(
                correlation_id=str(uuid4()), command_type='RETURN', status='PENDING',
                tool_id=tool['id'], loan_id=loan.id,
                branch_id=branch['id'], locker_id=locker['id'],
                compartment_id=free['id']))
            session.commit()
        conflict = client.post(f"/api/v1/compartments/{free['id']}/deactivate")
        assert conflict.status_code == 409
        conflict = client.patch(f"/api/v1/compartments/{free['id']}", json={'status': 'INACTIVE'})
        assert conflict.status_code == 409
