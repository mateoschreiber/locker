import json

from locker_simulator.protocol import PROTOCOL_VERSION, envelope, topic


def test_topic() -> None:
    assert topic("LAB-LOCKER-001", "status") == "locker/LAB-LOCKER-001/status"


def test_envelope_contains_required_fields() -> None:
    message = json.loads(envelope("LAB-LOCKER-001", "LOCKER_ONLINE", {"simulated": True}))

    assert message["protocol_version"] == PROTOCOL_VERSION
    assert message["locker_id"] == "LAB-LOCKER-001"
    assert message["type"] == "LOCKER_ONLINE"


def test_operation_event_keeps_correlation_and_compartment() -> None:
    message = json.loads(envelope("LAB-LOCKER-001", "COMPARTMENT_OPENED", {}, "abc", "C01"))

    assert message["correlation_id"] == "abc"
    assert message["compartment_id"] == "C01"
