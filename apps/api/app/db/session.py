from sqlalchemy import create_engine
from sqlalchemy.engine import Engine

from app.core.config import get_settings


def get_engine() -> Engine:
    return create_engine(get_settings().database_url, pool_pre_ping=True)
