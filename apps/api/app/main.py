from fastapi import FastAPI

from app.api.routes import api_router, router
from app.core.config import get_settings
from app.core.logging import configure_logging

settings = get_settings()
configure_logging(settings.log_level)

app = FastAPI(title="Locker API", version="0.1.0", description="Locker laboratory API")
app.include_router(router)
app.include_router(api_router)
