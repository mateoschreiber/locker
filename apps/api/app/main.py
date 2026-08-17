from fastapi import FastAPI

from app.api.routes import api_router, router
from app.core.config import get_settings
from app.core.logging import configure_logging
from app.db.session import SessionLocal
from app.modules.mqtt import mqtt_bridge
from app.modules.routes import router as domain_router
from app.modules.seed import seed_lab

settings = get_settings()
configure_logging(settings.log_level)

app = FastAPI(title="Locker API", version="0.1.0", description="Locker laboratory API")
app.include_router(router)
app.include_router(api_router)
app.include_router(domain_router)


@app.on_event("startup")
def initialize_lab() -> None:
    session = SessionLocal()
    try:
        seed_lab(session)
    finally:
        session.close()
    mqtt_bridge.start()


@app.on_event("shutdown")
def shutdown_mqtt() -> None:
    mqtt_bridge.stop()
