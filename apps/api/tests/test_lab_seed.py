from unittest.mock import patch

import pytest

from app.modules import seed


def test_seed_rejects_non_lab_before_connecting() -> None:
    with (
        patch("app.core.config.get_settings") as settings,
        patch("app.db.session.SessionLocal") as db,
        patch.dict("os.environ", {"APP_ENV": "production"}),
    ):
        settings.return_value.app_env = "production"
        with pytest.raises(RuntimeError, match="APP_ENV=lab"):
            seed.main()
        db.assert_not_called()


def test_startup_does_not_open_database() -> None:
    from app.main import initialize_lab

    with patch("app.main.mqtt_bridge.start") as start:
        initialize_lab()
        start.assert_called_once()
