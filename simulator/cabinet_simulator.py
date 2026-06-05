#!/usr/bin/env python3
"""
化成柜模拟器 - 增强版
支持通过环境变量配置，支持多种异常模拟
"""
import paho.mqtt.client as mqtt
import json
import time
import random
import threading
import os
import logging
from datetime import datetime
from dataclasses import dataclass, field
from typing import List, Dict, Optional
from enum import Enum

logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)


class AnomalyType(Enum):
    VOLTAGE_ABNORMAL = "voltage_abnormal"
    CURRENT_ABNORMAL = "current_abnormal"
    TEMPERATURE_HIGH = "temperature_high"
    CAPACITY_LOW = "capacity_low"
    NO_ANOMALY = "no_anomaly"


@dataclass
class SimulatorConfig:
    num_cabinets: int = 20
    channels_per_cabinet: int = 512
    rated_capacity: float = 3.2
    mqtt_broker: str = "localhost"
    mqtt_port: int = 1883
    report_interval: int = 10
    abnormal_ratio: float = 0.03
    anomaly_duration_min: int = 60
    anomaly_duration_max: int = 300
    mqtt_username: Optional[str] = None
    mqtt_password: Optional[str] = None
    mqtt_topic_template: str = "battery/cabinet/{cabinet_id}/data"
    mqtt_qos: int = 1
    batch_size: int = 64
    capacity_factor_min: float = 0.85
    capacity_factor_max: float = 1.05

    @classmethod
    def from_env(cls) -> "SimulatorConfig":
        return cls(
            num_cabinets=int(os.getenv("NUM_CABINETS", "20")),
            channels_per_cabinet=int(os.getenv("CHANNELS_PER_CABINET", "512")),
            rated_capacity=float(os.getenv("RATED_CAPACITY", "3.2")),
            mqtt_broker=os.getenv("MQTT_BROKER", "localhost"),
            mqtt_port=int(os.getenv("MQTT_PORT", "1883")),
            report_interval=int(os.getenv("REPORT_INTERVAL", "10")),
            abnormal_ratio=float(os.getenv("ABNORMAL_RATIO", "0.03")),
            anomaly_duration_min=int(os.getenv("ANOMALY_DURATION_MIN", "60")),
            anomaly_duration_max=int(os.getenv("ANOMALY_DURATION_MAX", "300")),
            mqtt_username=os.getenv("MQTT_USERNAME"),
            mqtt_password=os.getenv("MQTT_PASSWORD"),
            mqtt_topic_template=os.getenv("MQTT_TOPIC_TEMPLATE", "battery/cabinet/{cabinet_id}/data"),
            mqtt_qos=int(os.getenv("MQTT_QOS", "1")),
            batch_size=int(os.getenv("BATCH_SIZE", "64")),
            capacity_factor_min=float(os.getenv("CAPACITY_FACTOR_MIN", "0.85")),
            capacity_factor_max=float(os.getenv("CAPACITY_FACTOR_MAX", "1.05")),
        )


STAGES = ["precharge", "cc_charge", "cv_charge", "rest", "discharge"]

STAGE_DURATIONS = {
    "precharge": 1800,
    "cc_charge": 7200,
    "cv_charge": 3600,
    "rest": 1800,
    "discharge": 5400
}

STAGE_VOLTAGE_RANGES = {
    "precharge": (2.5, 3.0),
    "cc_charge": (3.0, 4.2),
    "cv_charge": (4.15, 4.2),
    "rest": (3.9, 4.1),
    "discharge": (3.8, 2.8)
}

STAGE_CURRENT_RANGES = {
    "precharge": (0.05, 0.1),
    "cc_charge": (1.5, 1.6),
    "cv_charge": (0.1, 1.5),
    "rest": (0.0, 0.0),
    "discharge": (-1.6, -1.5)
}


class ChannelSimulator:
    def __init__(self, cabinet_id: int, channel_id: int, config: SimulatorConfig):
        self.cabinet_id = cabinet_id
        self.channel_id = channel_id
        self.config = config

        self.cycle_index = 0
        self.current_stage = 0
        self.stage_elapsed = 0
        self.total_elapsed = 0
        self.capacity = 0.0

        self.base_voltage_offset = random.uniform(-0.05, 0.05)
        self.capacity_factor = random.uniform(
            config.capacity_factor_min,
            config.capacity_factor_max
        )

        self.anomaly_type: AnomalyType = AnomalyType.NO_ANOMALY
        self.anomaly_duration = 0
        self.anomaly_start_time = 0

        if random.random() < config.abnormal_ratio:
            anomaly_types = [
                AnomalyType.VOLTAGE_ABNORMAL,
                AnomalyType.CURRENT_ABNORMAL,
                AnomalyType.TEMPERATURE_HIGH,
                AnomalyType.CAPACITY_LOW
            ]
            self.anomaly_type = random.choice(anomaly_types)

    def _is_anomaly_active(self) -> bool:
        return self.anomaly_type != AnomalyType.NO_ANOMALY and self.anomaly_duration > 0

    def _maybe_trigger_anomaly(self):
        if self.anomaly_type != AnomalyType.NO_ANOMALY and self.anomaly_duration <= 0:
            if random.random() < 0.01:
                self.anomaly_duration = random.randint(
                    self.config.anomaly_duration_min,
                    self.config.anomaly_duration_max
                )
                self.anomaly_start_time = self.total_elapsed
                logger.info(
                    f"Cabinet {self.cabinet_id} Channel {self.channel_id}: "
                    f"Anomaly triggered: {self.anomaly_type.value}"
                )

    def _apply_voltage_anomaly(self, voltage: float) -> float:
        if self.anomaly_type == AnomalyType.VOLTAGE_ABNORMAL and self._is_anomaly_active():
            voltage += random.uniform(-0.3, 0.3)
        return voltage

    def _apply_current_anomaly(self, current: float) -> float:
        if self.anomaly_type == AnomalyType.CURRENT_ABNORMAL and self._is_anomaly_active():
            current += random.uniform(-0.5, 0.5)
        return current

    def _apply_temperature_anomaly(self, temp: float) -> float:
        if self.anomaly_type == AnomalyType.TEMPERATURE_HIGH and self._is_anomaly_active():
            temp += random.uniform(5.0, 15.0)
        return temp

    def _apply_capacity_anomaly(self, capacity: float) -> float:
        if self.anomaly_type == AnomalyType.CAPACITY_LOW and self._is_anomaly_active():
            capacity *= 0.7
        return capacity

    def get_voltage(self) -> float:
        stage = STAGES[self.current_stage]
        v_min, v_max = STAGE_VOLTAGE_RANGES[stage]
        progress = self.stage_elapsed / STAGE_DURATIONS[stage]

        if stage == "precharge":
            voltage = v_min + (v_max - v_min) * min(progress * 2, 1.0)
        elif stage == "cc_charge":
            voltage = v_min + (v_max - v_min) * progress
        elif stage == "cv_charge":
            voltage = v_max - 0.05 * (1 - progress)
        elif stage == "rest":
            voltage = 4.0 - 0.1 * progress
        elif stage == "discharge":
            voltage = 3.8 - 0.9 * progress + 0.1 * (1 - abs(progress - 0.5) * 2)

        voltage += self.base_voltage_offset
        voltage = self._apply_voltage_anomaly(voltage)

        return round(max(2.0, min(4.5, voltage)), 4)

    def get_current(self) -> float:
        stage = STAGES[self.current_stage]
        c_min, c_max = STAGE_CURRENT_RANGES[stage]
        progress = self.stage_elapsed / STAGE_DURATIONS[stage]

        if stage == "cv_charge":
            current = c_max - (c_max - c_min) * progress
        elif stage == "rest":
            current = 0.0
        else:
            current = random.uniform(c_min, c_max)

        current = self._apply_current_anomaly(current)
        return round(current, 4)

    def get_temperature(self) -> float:
        base_temp = 25.0
        stage = STAGES[self.current_stage]

        if stage in ["cc_charge", "cv_charge", "discharge"]:
            base_temp += 5.0 + 3.0 * (self.stage_elapsed / STAGE_DURATIONS[stage])

        base_temp += random.uniform(-1.0, 1.0)
        base_temp = self._apply_temperature_anomaly(base_temp)

        return round(base_temp, 2)

    def update_capacity(self) -> float:
        current = self.get_current()

        if current > 0:
            self.capacity += current * (self.config.report_interval / 3600) * self.capacity_factor
        elif current < 0:
            self.capacity += current * (self.config.report_interval / 3600)

        self.capacity = max(0.0, min(self.config.rated_capacity * 1.1, self.capacity))
        self.capacity = self._apply_capacity_anomaly(self.capacity)

        return round(self.capacity, 4)

    def step(self) -> Dict:
        self._maybe_trigger_anomaly()

        self.stage_elapsed += self.config.report_interval
        self.total_elapsed += self.config.report_interval

        if self.anomaly_duration > 0:
            self.anomaly_duration -= self.config.report_interval
            if self.anomaly_duration <= 0:
                logger.info(
                    f"Cabinet {self.cabinet_id} Channel {self.channel_id}: "
                    f"Anomaly cleared"
                )

        if self.stage_elapsed >= STAGE_DURATIONS[STAGES[self.current_stage]]:
            self.current_stage = (self.current_stage + 1) % len(STAGES)
            self.stage_elapsed = 0

            if self.current_stage == 0:
                self.cycle_index += 1
                self.capacity = 0.0

        return {
            "timestamp": datetime.now().isoformat(),
            "cabinet_id": self.cabinet_id,
            "channel_id": self.channel_id,
            "voltage": self.get_voltage(),
            "current": self.get_current(),
            "temperature": self.get_temperature(),
            "capacity": self.update_capacity(),
            "cycle_index": self.cycle_index,
            "stage": STAGES[self.current_stage],
            "stage_duration": self.stage_elapsed
        }


class CabinetSimulator:
    def __init__(self, cabinet_id: int, config: SimulatorConfig):
        self.cabinet_id = cabinet_id
        self.config = config
        self.channels: List[ChannelSimulator] = []
        self.client: Optional[mqtt.Client] = None
        self.running = False
        self.publish_count = 0
        self.error_count = 0
        self._lock = threading.Lock()

    def _on_connect(self, client, userdata, flags, rc):
        if rc == 0:
            logger.info(f"Cabinet {self.cabinet_id}: MQTT Connected successfully")
        else:
            logger.error(f"Cabinet {self.cabinet_id}: MQTT Connection failed with code {rc}")

    def _on_publish(self, client, userdata, mid):
        with self._lock:
            self.publish_count += 1

    def connect(self):
        client_id = f"cabinet_{self.cabinet_id}_{int(time.time())}"
        self.client = mqtt.Client(client_id)

        if self.config.mqtt_username and self.config.mqtt_password:
            self.client.username_pw_set(
                self.config.mqtt_username,
                self.config.mqtt_password
            )

        self.client.on_connect = self._on_connect
        self.client.on_publish = self._on_publish

        self.client.connect(
            self.config.mqtt_broker,
            self.config.mqtt_port,
            60
        )
        self.channels = [
            ChannelSimulator(self.cabinet_id, i, self.config)
            for i in range(self.config.channels_per_cabinet)
        ]

    def publish_data(self):
        topic = self.config.mqtt_topic_template.format(cabinet_id=self.cabinet_id)
        while self.running:
            start_time = time.time()
            batch_data = []

            for channel in self.channels:
                data = channel.step()
                batch_data.append(data)

                if len(batch_data) >= self.config.batch_size:
                    self._publish_batch(topic, batch_data)
                    batch_data = []

            if batch_data:
                self._publish_batch(topic, batch_data)

            elapsed = time.time() - start_time
            sleep_time = max(0, self.config.report_interval - elapsed)
            time.sleep(sleep_time)

    def _publish_batch(self, topic: str, batch_data: List[Dict]):
        try:
            payload = json.dumps(batch_data)
            result = self.client.publish(
                topic,
                payload,
                qos=self.config.mqtt_qos
            )
            if result.rc != mqtt.MQTT_ERR_SUCCESS:
                with self._lock:
                    self.error_count += 1
                logger.warning(
                    f"Cabinet {self.cabinet_id}: "
                    f"Publish error: {result.rc}"
                )
        except Exception as e:
            with self._lock:
                self.error_count += 1
            logger.error(
                f"Cabinet {self.cabinet_id}: "
                f"Publish exception: {e}"
            )

    def get_stats(self) -> Dict:
        with self._lock:
            return {
                "cabinet_id": self.cabinet_id,
                "channels": len(self.channels),
                "publish_count": self.publish_count,
                "error_count": self.error_count
            }

    def start(self):
        self.running = True
        self.thread = threading.Thread(target=self.publish_data, daemon=True)
        self.thread.start()
        logger.info(
            f"化成柜 {self.cabinet_id} 模拟器已启动，"
            f"包含 {self.config.channels_per_cabinet} 个通道"
        )

    def stop(self):
        self.running = False
        if self.thread:
            self.thread.join(timeout=5)
        if self.client:
            self.client.disconnect()
        logger.info(f"化成柜 {self.cabinet_id} 模拟器已停止")


class SimulatorManager:
    def __init__(self, config: SimulatorConfig):
        self.config = config
        self.simulators: List[CabinetSimulator] = []
        self.stats_thread: Optional[threading.Thread] = None
        self.running = False

    def start(self):
        logger.info("=" * 60)
        logger.info("锂电池化成柜模拟器 - 增强版")
        logger.info("=" * 60)
        logger.info(f"配置信息:")
        logger.info(f"  化成柜数量: {self.config.num_cabinets}")
        logger.info(f"  每柜通道数: {self.config.channels_per_cabinet}")
        logger.info(f"  总通道数: {self.config.num_cabinets * self.config.channels_per_cabinet}")
        logger.info(f"  额定容量: {self.config.rated_capacity} Ah")
        logger.info(f"  上报间隔: {self.config.report_interval} 秒")
        logger.info(f"  异常比例: {self.config.abnormal_ratio * 100:.1f}%")
        logger.info(f"  MQTT Broker: {self.config.mqtt_broker}:{self.config.mqtt_port}")
        logger.info("=" * 60)

        for i in range(self.config.num_cabinets):
            sim = CabinetSimulator(i, self.config)
            sim.connect()
            sim.start()
            self.simulators.append(sim)
            time.sleep(0.1)

        self.running = True
        self.stats_thread = threading.Thread(target=self._print_stats, daemon=True)
        self.stats_thread.start()

        logger.info(f"\n所有 {self.config.num_cabinets} 台化成柜模拟器已启动")
        logger.info("按 Ctrl+C 停止...\n")

    def _print_stats(self):
        while self.running:
            time.sleep(60)
            total_publish = 0
            total_errors = 0
            for sim in self.simulators:
                stats = sim.get_stats()
                total_publish += stats["publish_count"]
                total_errors += stats["error_count"]

            logger.info(
                f"统计 - 总发布: {total_publish}, "
                f"总错误: {total_errors}"
            )

    def stop(self):
        logger.info("\n正在停止模拟器...")
        self.running = False
        for sim in self.simulators:
            sim.stop()
        if self.stats_thread and self.stats_thread.is_alive():
            self.stats_thread.join(timeout=5)
        logger.info("所有模拟器已停止")


def main():
    config = SimulatorConfig.from_env()
    manager = SimulatorManager(config)

    try:
        manager.start()
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        manager.stop()
    except Exception as e:
        logger.error(f"模拟器异常: {e}")
        manager.stop()


if __name__ == "__main__":
    main()
