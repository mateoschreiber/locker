from functools import lru_cache

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    app_env: str = Field(default="lab", validation_alias="APP_ENV")
    database_url: str = Field(
        default="postgresql+psycopg://locker:locker_lab_only_change_me@postgres:5432/locker_lab",
        validation_alias="DATABASE_URL",
    )
    mqtt_host: str = Field(default="mosquitto", validation_alias="MQTT_HOST")
    mqtt_port: int = Field(default=1883, validation_alias="MQTT_PORT")
    log_level: str = Field(default="INFO", validation_alias="LOG_LEVEL")
    timezone: str = Field(default="America/Asuncion", validation_alias="TIMEZONE")


@lru_cache
def get_settings() -> Settings:
    return Settings()
