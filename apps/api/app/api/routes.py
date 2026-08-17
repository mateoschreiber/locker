import socket
from contextlib import closing

from fastapi import APIRouter, HTTPException, status
from sqlalchemy import text
from sqlalchemy.exc import SQLAlchemyError

from app.core.config import get_settings
from app.db.session import get_engine

router = APIRouter()
api_router = APIRouter(prefix="/api/v1")


@router.get("/health/live", tags=["health"])
def liveness() -> dict[str, str]:
    return {"status": "ok"}


@router.get("/health/ready", tags=["health"])
def readiness() -> dict[str, str]:
    settings = get_settings()
    try:
        with get_engine().connect() as connection:
            connection.execute(text("SELECT 1"))
        with closing(socket.create_connection((settings.mqtt_host, settings.mqtt_port), timeout=2)):
            pass
    except (OSError, SQLAlchemyError) as error:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Laboratory dependencies are unavailable",
        ) from error
    return {"status": "ready"}


@api_router.get("/system/info", tags=["system"])
def system_info() -> dict[str, str]:
    settings = get_settings()
    return {
        "name": "Locker Lab",
        "version": "0.1.0",
        "environment": settings.app_env,
        "timezone": settings.timezone,
        "status": "bootstrapped",
    }
