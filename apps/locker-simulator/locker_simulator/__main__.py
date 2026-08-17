import json
import logging
import os
import time
from typing import Any

import paho.mqtt.client as mqtt

from locker_simulator.protocol import envelope, topic

logging.basicConfig(
    level=os.getenv("LOG_LEVEL", "INFO"),
    format="%(asctime)s %(levelname)s %(name)s %(message)s",
)
logger = logging.getLogger("locker_simulator")

LOCKER_ID = os.getenv("LOCKER_ID", "LAB-LOCKER-001")
MQTT_HOST = os.getenv("MQTT_HOST", "mosquitto")
MQTT_PORT = int(os.getenv("MQTT_PORT", "1883"))


def publish_status(client: mqtt.Client, event_type: str) -> None:
    client.publish(
        topic(LOCKER_ID, "status"),
        envelope(LOCKER_ID, event_type, {"simulated": True}),
        qos=1,
        retain=True,
    )


def on_connect(
    client: mqtt.Client,
    userdata: Any,
    flags: mqtt.ConnectFlags,
    reason_code: mqtt.ReasonCode,
    properties: mqtt.Properties | None,
) -> None:
    del userdata, flags, properties
    if reason_code.is_failure:
        logger.error("mqtt_connection_refused reason=%s", reason_code)
        return
    client.subscribe(topic(LOCKER_ID, "commands"), qos=1)
    publish_status(client, "LOCKER_ONLINE")
    logger.info("simulator_online locker_id=%s", LOCKER_ID)


def on_message(client: mqtt.Client, userdata: Any, message: mqtt.MQTTMessage) -> None:
    del client, userdata
    try:
        command = json.loads(message.payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        logger.warning("invalid_command topic=%s", message.topic)
        return
    logger.info("diagnostic_command_received topic=%s command=%s", message.topic, command)


def main() -> None:
    while True:
        client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id=f"simulator-{LOCKER_ID}")
        client.on_connect = on_connect
        client.on_message = on_message
        try:
            client.connect(MQTT_HOST, MQTT_PORT, keepalive=30)
            client.loop_start()
            while client.is_connected():
                publish_status(client, "LOCKER_HEARTBEAT")
                time.sleep(30)
            client.loop_stop()
        except OSError as error:
            logger.warning("mqtt_unavailable host=%s error=%s", MQTT_HOST, error)
        finally:
            client.disconnect()
        time.sleep(3)


if __name__ == "__main__":
    main()
