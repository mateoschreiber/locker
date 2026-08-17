from fastapi.testclient import TestClient

from app.main import app

client = TestClient(app)


def test_liveness() -> None:
    response = client.get("/health/live")

    assert response.status_code == 200
    assert response.json() == {"status": "ok"}


def test_system_info_has_no_secrets() -> None:
    response = client.get("/api/v1/system/info")

    assert response.status_code == 200
    assert response.json()["name"] == "Locker Lab"
    assert "password" not in response.text.lower()
