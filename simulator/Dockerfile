FROM python:3.11-slim

WORKDIR /app

ENV PYTHONDONTWRITEBYTECODE=1
ENV PYTHONUNBUFFERED=1
ENV TZ=Asia/Shanghai

RUN apt-get update && apt-get install -y \
    tzdata \
    && rm -rf /var/lib/apt/lists/*

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY cabinet_simulator.py .

HEALTHCHECK --interval=30s --timeout=3s --retries=3 \
    CMD python -c "import paho.mqtt.client as mqtt; c=mqtt.Client(); c.connect('${MQTT_BROKER:-localhost}', int('${MQTT_PORT:-1883}'), 5); print('OK')" || exit 1

CMD ["python", "cabinet_simulator.py"]
