from types import SimpleNamespace
from unittest.mock import Mock, patch

from fastapi.testclient import TestClient

from app.main import app
from app.modules.mqtt import MqttBridge


def test_suback_and_disconnect_control_readiness() -> None:
    bridge = MqttBridge()
    bridge.subscription_mid = 42
    good = [SimpleNamespace(is_failure=False)]
    bad = [SimpleNamespace(is_failure=True)]
    bridge._on_subscribe(Mock(), None, 41, good, None)
    assert not bridge.subscribed.is_set()
    bridge._on_subscribe(Mock(), None, 42, bad, None)
    assert not bridge.subscribed.is_set()
    bridge._on_subscribe(Mock(), None, 42, good, None)
    assert bridge.subscribed.is_set()
    bridge._on_disconnect(Mock(), None, None, None, None)
    assert not bridge.subscribed.is_set()


def test_readiness_requires_mqtt_subscription() -> None:
    with (
        patch('app.api.routes.get_engine'),
        patch('app.api.routes.socket.create_connection'),
        patch('app.api.routes.mqtt_bridge.subscribed.is_set', return_value=False) as ready,
    ):
        client = TestClient(app)
        assert client.get('/health/ready').status_code == 503
        ready.return_value = True
        assert client.get('/health/ready').status_code == 200
