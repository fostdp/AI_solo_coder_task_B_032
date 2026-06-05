import paho.mqtt.client as mqtt
import json
import time
import random
import threading
from datetime import datetime

NUM_CABINETS = 20
CHANNELS_PER_CABINET = 512
RATED_CAPACITY = 3.2
MQTT_BROKER = "localhost"
MQTT_PORT = 1883
REPORT_INTERVAL = 10

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
    def __init__(self, cabinet_id, channel_id):
        self.cabinet_id = cabinet_id
        self.channel_id = channel_id
        self.cycle_index = 0
        self.current_stage = 0
        self.stage_elapsed = 0
        self.total_elapsed = 0
        self.capacity = 0.0
        self.base_voltage_offset = random.uniform(-0.05, 0.05)
        self.capacity_factor = random.uniform(0.85, 1.05)
        self.is_abnormal = random.random() < 0.03
        self.anomaly_start = 0
        self.anomaly_duration = 0
        
    def get_voltage(self):
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
        
        if self.is_abnormal and self.anomaly_duration > 0:
            voltage += random.uniform(-0.3, 0.3)
        
        return round(max(2.0, min(4.5, voltage)), 4)
    
    def get_current(self):
        stage = STAGES[self.current_stage]
        c_min, c_max = STAGE_CURRENT_RANGES[stage]
        progress = self.stage_elapsed / STAGE_DURATIONS[stage]
        
        if stage == "cv_charge":
            current = c_max - (c_max - c_min) * progress
        elif stage == "rest":
            current = 0.0
        else:
            current = random.uniform(c_min, c_max)
        
        if self.is_abnormal and self.anomaly_duration > 0:
            current += random.uniform(-0.5, 0.5)
        
        return round(current, 4)
    
    def get_temperature(self):
        base_temp = 25.0
        stage = STAGES[self.current_stage]
        
        if stage in ["cc_charge", "cv_charge", "discharge"]:
            base_temp += 5.0 + 3.0 * (self.stage_elapsed / STAGE_DURATIONS[stage])
        
        base_temp += random.uniform(-1.0, 1.0)
        
        if self.is_abnormal and self.anomaly_duration > 0:
            base_temp += random.uniform(5.0, 15.0)
        
        return round(base_temp, 2)
    
    def update_capacity(self):
        stage = STAGES[self.current_stage]
        current = self.get_current()
        
        if current > 0:
            self.capacity += current * (REPORT_INTERVAL / 3600) * self.capacity_factor
        elif current < 0:
            self.capacity += current * (REPORT_INTERVAL / 3600)
        
        self.capacity = max(0.0, min(RATED_CAPACITY * 1.1, self.capacity))
        
        return round(self.capacity, 4)
    
    def step(self):
        self.stage_elapsed += REPORT_INTERVAL
        self.total_elapsed += REPORT_INTERVAL
        
        if self.is_abnormal:
            if self.anomaly_duration <= 0 and random.random() < 0.01:
                self.anomaly_duration = random.randint(60, 300)
            elif self.anomaly_duration > 0:
                self.anomaly_duration -= REPORT_INTERVAL
        
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
    def __init__(self, cabinet_id):
        self.cabinet_id = cabinet_id
        self.channels = [ChannelSimulator(cabinet_id, i) for i in range(CHANNELS_PER_CABINET)]
        self.client = mqtt.Client(f"cabinet_{cabinet_id}")
        self.client.connect(MQTT_BROKER, MQTT_PORT, 60)
        self.running = False
    
    def publish_data(self):
        topic = f"battery/cabinet/{self.cabinet_id}/data"
        while self.running:
            start_time = time.time()
            batch_data = []
            
            for channel in self.channels:
                data = channel.step()
                batch_data.append(data)
                
                if len(batch_data) >= 64:
                    payload = json.dumps(batch_data)
                    self.client.publish(topic, payload)
                    batch_data = []
            
            if batch_data:
                payload = json.dumps(batch_data)
                self.client.publish(topic, payload)
            
            elapsed = time.time() - start_time
            sleep_time = max(0, REPORT_INTERVAL - elapsed)
            time.sleep(sleep_time)
    
    def start(self):
        self.running = True
        self.thread = threading.Thread(target=self.publish_data)
        self.thread.daemon = True
        self.thread.start()
        print(f"化成柜 {self.cabinet_id} 模拟器已启动，包含 {CHANNELS_PER_CABINET} 个通道")
    
    def stop(self):
        self.running = False
        if self.thread:
            self.thread.join()
        self.client.disconnect()
        print(f"化成柜 {self.cabinet_id} 模拟器已停止")

def main():
    simulators = []
    
    try:
        for i in range(NUM_CABINETS):
            sim = CabinetSimulator(i)
            sim.start()
            simulators.append(sim)
            time.sleep(0.1)
        
        print(f"\n所有 {NUM_CABINETS} 台化成柜模拟器已启动")
        print(f"每台 {CHANNELS_PER_CABINET} 通道，共 {NUM_CABINETS * CHANNELS_PER_CABINET} 通道")
        print(f"每 {REPORT_INTERVAL} 秒上报一次数据")
        print("按 Ctrl+C 停止...\n")
        
        while True:
            time.sleep(1)
    
    except KeyboardInterrupt:
        print("\n正在停止模拟器...")
        for sim in simulators:
            sim.stop()
        print("所有模拟器已停止")

if __name__ == "__main__":
    main()
