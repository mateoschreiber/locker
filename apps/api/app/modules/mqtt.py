"""MQTT bridge for the laboratory simulator."""

import json
import logging
from datetime import UTC, datetime
from typing import Any
from uuid import uuid4

import paho.mqtt.client as mqtt
from sqlalchemy import select

from app.core.config import get_settings
from app.db.session import SessionLocal
from app.modules.models import AuditEvent, Loan, LockerOperation, Tool, ToolPlacement

logger = logging.getLogger("locker.mqtt")


def command_envelope(locker_id: str, compartment_id: str, correlation_id: str) -> str:
    return json.dumps(
        {
            "message_id": str(uuid4()),
            "correlation_id": correlation_id,
            "protocol_version": "1.0",
            "locker_id": locker_id,
            "compartment_id": compartment_id,
            "timestamp": datetime.now(UTC).isoformat(),
            "type": "OPEN_COMPARTMENT",
            "payload": {"source": "locker-api"},
        },
        separators=(",", ":"),
    )


class MqttBridge:
    def __init__(self) -> None:
        settings = get_settings()
        self.host, self.port = settings.mqtt_host, settings.mqtt_port
        self.client = mqtt.Client(
            mqtt.CallbackAPIVersion.VERSION2,  # type: ignore[attr-defined]
            client_id="locker-api",
        )
        self.client.on_connect, self.client.on_message = self._on_connect, self._on_message

    def start(self) -> None:
        self.client.connect_async(self.host, self.port, keepalive=30)
        self.client.loop_start()

    def stop(self) -> None:
        self.client.loop_stop()
        self.client.disconnect()

    def publish_open(self, locker_code: str, compartment_id: str, correlation_id: str) -> None:
        self.client.publish(
            f"locker/{locker_code}/commands",
            command_envelope(locker_code, compartment_id, correlation_id),
            qos=1,
        )

    def _on_connect(
        self,
        client: mqtt.Client,
        userdata: Any,
        flags: mqtt.ConnectFlags,
        reason_code: Any,
        properties: Any,
    ) -> None:
        del userdata, flags, properties
        if not reason_code.is_failure:
            client.subscribe("locker/+/events", qos=1)
            logger.info("mqtt_events_subscribed")

    def _on_message(self, client: mqtt.Client, userdata: Any, message: mqtt.MQTTMessage) -> None:
        del client, userdata
        try:
            payload = json.loads(message.payload.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            logger.warning("mqtt_invalid_event topic=%s", message.topic)
            return
        if payload.get("type") == "COMPARTMENT_OPENED" and payload.get("correlation_id"):
            self.confirm(payload)

    def confirm(self, payload: dict[str, Any]) -> None:
        correlation_id = str(payload["correlation_id"])
        with SessionLocal() as session:
            operation = session.scalar(
                select(LockerOperation).where(LockerOperation.correlation_id == correlation_id)
            )
            if operation is None or operation.status != "PENDING":
                return
            now = datetime.now(UTC)
            loan, tool = session.get(Loan, operation.loan_id), session.get(Tool, operation.tool_id)
            if (
                str(operation.compartment_id) != str(payload.get("compartment_id"))
                or loan is None
                or tool is None
            ):
                operation.status, operation.failure_reason = (
                    "FAILED",
                    "Command data does not match event",
                )
            elif operation.command_type == "CHECKOUT" and loan.status == "CHECKOUT_PENDING":
                placement = session.scalar(
                    select(ToolPlacement).where(
                        ToolPlacement.tool_id == operation.tool_id,
                        ToolPlacement.removed_at.is_(None),
                    )
                )
                if placement is None:
                    operation.status, operation.failure_reason = (
                        "FAILED",
                        "Active placement not found",
                    )
                else:
                    (
                        placement.removed_at,
                        tool.status,
                        loan.status,
                        loan.checked_out_at,
                        operation.status,
                    ) = now, "ON_LOAN", "ACTIVE", now, "CONFIRMED"
            elif operation.command_type == "RETURN" and loan.status == "RETURN_PENDING":
                occupied = session.scalar(
                    select(ToolPlacement).where(
                        ToolPlacement.compartment_id == operation.compartment_id,
                        ToolPlacement.removed_at.is_(None),
                    )
                )
                if occupied is not None:
                    operation.status, operation.failure_reason = "FAILED", "Compartment is occupied"
                else:
                    session.add(
                        ToolPlacement(
                            tool_id=operation.tool_id,
                            branch_id=operation.branch_id,
                            locker_id=operation.locker_id,
                            compartment_id=operation.compartment_id,
                            reason="LOAN_RETURN",
                        )
                    )
                    tool.status, loan.status, loan.returned_at, operation.status = (
                        "AVAILABLE",
                        "RETURNED",
                        now,
                        "CONFIRMED",
                    )
            else:
                operation.status, operation.failure_reason = "FAILED", "Unexpected loan state"
            operation.confirmed_at = now
            session.add(
                AuditEvent(
                    action=f"MQTT_{operation.status}",
                    entity_type="LockerOperation",
                    entity_id=str(operation.id),
                    metadata_json={"correlation_id": correlation_id},
                )
            )
            session.commit()


mqtt_bridge = MqttBridge()
