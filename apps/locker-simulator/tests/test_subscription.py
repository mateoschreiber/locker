import json
from types import SimpleNamespace
from unittest.mock import Mock, patch

from locker_simulator import __main__ as simulator
from locker_simulator.__main__ import on_message, on_subscribe, subscribed


def open_command(correlation_id: str) -> SimpleNamespace:
    payload = json.dumps(
        {
            "message_id": "m",
            "correlation_id": correlation_id,
            "protocol_version": "1.0",
            "locker_id": "LAB-LOCKER-001",
            "compartment_id": "c",
            "timestamp": "2026-11-19T00:00:00+00:00",
            "type": "OPEN_COMPARTMENT",
            "payload": {"source": "locker-api-rust"},
        }
    ).encode()
    return SimpleNamespace(payload=payload, topic="locker/LAB-LOCKER-001/commands")


def test_simulator_announces_only_after_successful_suback() -> None:
    subscribed.clear()
    client = Mock()
    with patch('locker_simulator.__main__.publish_status') as publish:
        on_subscribe(client, None, 1, [SimpleNamespace(is_failure=True)], None)
        assert not subscribed.is_set()
        publish.assert_not_called()
        on_subscribe(client, None, 1, [SimpleNamespace(is_failure=False)], None)
        assert subscribed.is_set()
        publish.assert_called_once_with(client, 'LOCKER_ONLINE')
    subscribed.clear()


def test_duplicate_open_command_is_ignored() -> None:
    client = Mock()
    message = open_command("dedup-test-1")
    on_message(client, None, message)
    on_message(client, None, message)
    client.publish.assert_called_once()


def test_dedup_window_is_bounded_but_keeps_recent_ids(monkeypatch) -> None:
    """The dedup cache keeps the last SEEN_LIMIT correlations: ids evicted
    from the window are answered again, recent ones stay deduplicated."""
    monkeypatch.setattr(simulator, "SEEN_LIMIT", 3)
    simulator.seen_correlations.clear()
    simulator.seen_lookup.clear()
    client = Mock()
    on_message(client, None, open_command("old-1"))
    on_message(client, None, open_command("old-2"))
    on_message(client, None, open_command("recent-1"))
    on_message(client, None, open_command("recent-2"))
    assert client.publish.call_count == 4
    # "old-1" fell out of the 3-entry window, so a redelivery is answered
    # again; the recent ids are still inside the window and deduplicated.
    on_message(client, None, open_command("old-1"))
    assert client.publish.call_count == 5
    on_message(client, None, open_command("recent-2"))
    assert client.publish.call_count == 5
    simulator.seen_correlations.clear()
    simulator.seen_lookup.clear()


def test_dedup_window_is_lost_on_restart(monkeypatch) -> None:
    """After a restart the window starts empty: a redelivered command from
    before the restart is answered again (documented, not a persistent
    guarantee; persistent dedup is a requirement for real hardware)."""
    monkeypatch.setattr(simulator, "SEEN_LIMIT", 10)
    simulator.seen_correlations.clear()
    simulator.seen_lookup.clear()
    client = Mock()
    on_message(client, None, open_command("before-restart"))
    assert client.publish.call_count == 1
    simulator.seen_correlations.clear()
    simulator.seen_lookup.clear()
    on_message(client, None, open_command("before-restart"))
    assert client.publish.call_count == 2
