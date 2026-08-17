import json
from datetime import UTC, datetime
from typing import Any
from uuid import uuid4

PROTOCOL_VERSION = "1.0"


def topic(locker_id: str, channel: str) -> str:
    return f"locker/{locker_id}/{channel}"


def envelope(locker_id: str, event_type: str, payload: dict[str, Any]) -> str:
    return json.dumps(
        {
            "message_id": str(uuid4()),
            "correlation_id": None,
            "protocol_version": PROTOCOL_VERSION,
            "locker_id": locker_id,
            "compartment_id": None,
            "timestamp": datetime.now(UTC).isoformat(),
            "type": event_type,
            "payload": payload,
        },
        separators=(",", ":"),
    )
