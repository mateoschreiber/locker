FROM python:3.12-slim

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1

WORKDIR /app
COPY pyproject.toml ./
COPY locker_simulator ./locker_simulator
RUN pip install .

CMD ["python", "-m", "locker_simulator"]
