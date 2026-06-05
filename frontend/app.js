const API_BASE = 'http://localhost:8080/api';

let currentCabinetId = 0;
let autoRefresh = true;
let refreshInterval = null;
let currentChannelData = null;
let channelDataCache = {};
let blinkChannels = new Set();

const STAGE_NAMES = {
    1: '预充',
    2: '恒流充电',
    3: '恒压充电',
    4: '搁置',
    5: '放电'
};

const STAGE_COLORS = {
    1: '#9966FF',
    2: '#00CCFF',
    3: '#FF9900',
    4: '#666666',
    5: '#FF3366'
};

document.addEventListener('DOMContentLoaded', () => {
    initCabinetList();
    loadSystemStats();
    loadAlerts();
    loadCabinetPanel(0);
    
    refreshInterval = setInterval(() => {
        if (autoRefresh) {
            loadCabinetPanel(currentCabinetId);
            loadSystemStats();
            loadAlerts();
        }
    }, 5000);

    setupCanvasEvents();
    setupModalEvents();
});

function initCabinetList() {
    const container = document.getElementById('cabinetList');
    container.innerHTML = '';
    
    for (let i = 0; i < 20; i++) {
        const btn = document.createElement('button');
        btn.className = 'cabinet-btn' + (i === 0 ? ' active' : '');
        btn.innerHTML = `
            <span>${i}</span>
            <span class="abnormal-badge" id="abnormal-badge-${i}" style="display:none;">0</span>
        `;
        btn.onclick = () => selectCabinet(i);
        container.appendChild(btn);
    }
}

function selectCabinet(id) {
    currentCabinetId = id;
    
    document.querySelectorAll('.cabinet-btn').forEach((btn, idx) => {
        btn.classList.toggle('active', idx === id);
    });
    
    document.getElementById('currentCabinetTitle').textContent = `化成柜 ${id} - 实时监控`;
    loadCabinetPanel(id);
}

async function loadSystemStats() {
    try {
        const response = await fetch(`${API_BASE}/stats/summary`);
        const result = await response.json();
        
        if (result.success && result.data) {
            const data = result.data;
            
            document.getElementById('totalCabinets').textContent = data.total_cabinets;
            document.getElementById('totalChannels').textContent = data.active_channels.toLocaleString();
            document.getElementById('abnormalChannels').textContent = data.abnormal_channels;
            document.getElementById('avgCapacity').textContent = (data.avg_capacity_ratio * 100).toFixed(1) + '%';
            
            data.cabinets.forEach(cab => {
                const badge = document.getElementById(`abnormal-badge-${cab.cabinet_id}`);
                if (badge) {
                    if (cab.abnormal_channels > 0) {
                        badge.style.display = 'flex';
                        badge.textContent = cab.abnormal_channels;
                    } else {
                        badge.style.display = 'none';
                    }
                }
            });
        }
    } catch (e) {
        console.error('Failed to load system stats:', e);
    }
}

async function loadAlerts() {
    try {
        const response = await fetch(`${API_BASE}/alerts?limit=20`);
        const result = await response.json();
        
        if (result.success && result.data) {
            const container = document.getElementById('alertsList');
            container.innerHTML = '';
            
            result.data.forEach(alert => {
                const item = document.createElement('div');
                item.className = `alert-item level${alert.alert_level}`;
                
                const time = new Date(alert.timestamp).toLocaleString('zh-CN');
                item.innerHTML = `
                    <div>${alert.message}</div>
                    <div class="alert-time">${time}</div>
                `;
                
                item.onclick = () => {
                    if (alert.cabinet_id !== undefined) {
                        selectCabinet(alert.cabinet_id);
                    }
                };
                
                container.appendChild(item);
            });
            
            if (result.data.length === 0) {
                container.innerHTML = '<div style="color: #666; font-size: 12px; text-align: center;">暂无告警</div>';
            }
        }
    } catch (e) {
        console.error('Failed to load alerts:', e);
    }
}

async function loadCabinetPanel(cabinetId) {
    try {
        const response = await fetch(`${API_BASE}/cabinet/${cabinetId}`);
        const result = await response.json();
        
        if (result.success && result.data) {
            channelDataCache[cabinetId] = result.data;
            renderCabinetPanel(result.data);
            renderCabinetStats(result.data.stats);
        }
    } catch (e) {
        console.error('Failed to load cabinet panel:', e);
        renderMockData(cabinetId);
    }
}

function renderMockData(cabinetId) {
    const mockData = {
        cabinet_id: cabinetId,
        channels: [],
        stats: {
            avg_voltage: 3.8,
            std_voltage: 0.05,
            avg_current: 0.5,
            avg_temperature: 30,
            abnormal_channel_count: 5,
            total_channels: 512,
            abnormal_ratio: 0.01
        }
    };

    for (let i = 0; i < 512; i++) {
        const ratio = 0.85 + Math.random() * 0.15;
        mockData.channels.push({
            channel_id: i,
            capacity_ratio: ratio,
            is_abnormal: Math.random() < 0.02,
            is_paused: Math.random() < 0.01,
            stage: Math.floor(Math.random() * 5) + 1,
            color: getChannelColor(ratio, Math.random() < 0.02, Math.random() < 0.01)
        });
    }

    channelDataCache[cabinetId] = mockData;
    renderCabinetPanel(mockData);
    renderCabinetStats(mockData.stats);
}

function getChannelColor(ratio, isAbnormal, isPaused) {
    if (isPaused) return '#808080';
    if (isAbnormal) return '#FF0000';
    if (ratio >= 0.95) return '#00FF00';
    if (ratio >= 0.90) return '#FFFF00';
    return '#FF6600';
}

function renderCabinetPanel(data) {
    const canvas = document.getElementById('cabinetCanvas');
    const ctx = canvas.getContext('2d');
    
    const padding = 20;
    const cols = 32;
    const rows = 16;
    const totalChannels = cols * rows;
    
    const availableWidth = canvas.width - padding * 2;
    const availableHeight = canvas.height - padding * 2;
    
    const cellWidth = Math.floor(availableWidth / cols) - 2;
    const cellHeight = Math.floor(availableHeight / rows) - 2;
    
    ctx.fillStyle = '#0a0a1a';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    
    const gridStartX = padding + (availableWidth - (cols * (cellWidth + 2))) / 2;
    const gridStartY = padding + (availableHeight - (rows * (cellHeight + 2))) / 2;
    
    const blinkTime = Date.now() % 1000 < 500;
    
    data.channels.forEach((channel, idx) => {
        const col = idx % cols;
        const row = Math.floor(idx / cols);
        
        const x = gridStartX + col * (cellWidth + 2);
        const y = gridStartY + row * (cellHeight + 2);
        
        let color = channel.color;
        
        if (channel.is_abnormal && blinkTime) {
            color = '#FFFFFF';
        }
        
        ctx.fillStyle = color;
        ctx.fillRect(x, y, cellWidth, cellHeight);
        
        if (channel.is_abnormal && !blinkTime) {
            ctx.strokeStyle = '#FFFFFF';
            ctx.lineWidth = 1;
            ctx.strokeRect(x, y, cellWidth, cellHeight);
        }
    });
    
    ctx.fillStyle = '#666';
    ctx.font = '12px sans-serif';
    ctx.textAlign = 'center';
    for (let i = 0; i < cols; i += 4) {
        ctx.fillText(i.toString(), gridStartX + i * (cellWidth + 2) + cellWidth / 2, 15);
    }
    
    ctx.textAlign = 'right';
    for (let i = 0; i < rows; i += 2) {
        ctx.fillText((i * cols).toString(), gridStartX - 5, gridStartY + i * (cellHeight + 2) + cellHeight / 2 + 4);
    }
}

function renderCabinetStats(stats) {
    if (!stats) return;
    
    const container = document.getElementById('cabinetStats');
    container.innerHTML = `
        <div class="stat-bar-item">
            <span class="label">平均电压</span>
            <span class="value" style="color: #00d9ff;">${stats.avg_voltage.toFixed(3)} V</span>
        </div>
        <div class="stat-bar-item">
            <span class="label">电压标准差</span>
            <span class="value" style="color: ${stats.std_voltage > 0.1 ? '#ff4444' : '#00ff00'};">${stats.std_voltage.toFixed(4)} V</span>
        </div>
        <div class="stat-bar-item">
            <span class="label">平均电流</span>
            <span class="value" style="color: #00d9ff;">${stats.avg_current.toFixed(3)} A</span>
        </div>
        <div class="stat-bar-item">
            <span class="label">平均温度</span>
            <span class="value" style="color: ${stats.avg_temperature > 40 ? '#ff4444' : '#00ff00'};">${stats.avg_temperature.toFixed(1)} °C</span>
        </div>
        <div class="stat-bar-item">
            <span class="label">异常通道</span>
            <span class="value" style="color: #ff4444;">${stats.abnormal_channel_count} / ${stats.total_channels}</span>
        </div>
        <div class="stat-bar-item">
            <span class="label">异常率</span>
            <span class="value" style="color: ${stats.abnormal_ratio > 0.05 ? '#ff4444' : '#00ff00'};">${(stats.abnormal_ratio * 100).toFixed(1)}%</span>
        </div>
    `;
}

function setupCanvasEvents() {
    const canvas = document.getElementById('cabinetCanvas');
    const overlay = document.getElementById('canvasOverlay');
    
    canvas.addEventListener('click', (e) => {
        const rect = canvas.getBoundingClientRect();
        const scaleX = canvas.width / rect.width;
        const scaleY = canvas.height / rect.height;
        
        const x = (e.clientX - rect.left) * scaleX;
        const y = (e.clientY - rect.top) * scaleY;
        
        const channelId = getChannelFromPosition(x, y);
        if (channelId !== null) {
            showChannelDetail(currentCabinetId, channelId);
        }
    });
    
    canvas.addEventListener('mousemove', (e) => {
        const rect = canvas.getBoundingClientRect();
        const scaleX = canvas.width / rect.width;
        const scaleY = canvas.height / rect.height;
        
        const x = (e.clientX - rect.left) * scaleX;
        const y = (e.clientY - rect.top) * scaleY;
        
        const channelId = getChannelFromPosition(x, y);
        if (channelId !== null) {
            showTooltip(e.clientX, e.clientY, channelId);
        } else {
            hideTooltip();
        }
    });
    
    canvas.addEventListener('mouseleave', hideTooltip);
}

function getChannelFromPosition(x, y) {
    const data = channelDataCache[currentCabinetId];
    if (!data) return null;
    
    const canvas = document.getElementById('cabinetCanvas');
    const padding = 20;
    const cols = 32;
    const rows = 16;
    
    const availableWidth = canvas.width - padding * 2;
    const availableHeight = canvas.height - padding * 2;
    
    const cellWidth = Math.floor(availableWidth / cols) - 2;
    const cellHeight = Math.floor(availableHeight / rows) - 2;
    
    const gridStartX = padding + (availableWidth - (cols * (cellWidth + 2))) / 2;
    const gridStartY = padding + (availableHeight - (rows * (cellHeight + 2))) / 2;
    
    const col = Math.floor((x - gridStartX) / (cellWidth + 2));
    const row = Math.floor((y - gridStartY) / (cellHeight + 2));
    
    if (col >= 0 && col < cols && row >= 0 && row < rows) {
        const channelId = row * cols + col;
        if (channelId < 512) {
            return channelId;
        }
    }
    
    return null;
}

function showTooltip(x, y, channelId) {
    let tooltip = document.querySelector('.tooltip');
    if (!tooltip) {
        tooltip = document.createElement('div');
        tooltip.className = 'tooltip';
        document.body.appendChild(tooltip);
    }
    
    const data = channelDataCache[currentCabinetId];
    const channel = data?.channels[channelId];
    
    if (channel) {
        const ratio = (channel.capacity_ratio * 100).toFixed(1);
        const stageName = STAGE_NAMES[channel.stage] || '未知';
        
        tooltip.innerHTML = `
            <strong>通道 ${channelId}</strong><br>
            容量比: ${ratio}%<br>
            阶段: ${stageName}<br>
            状态: ${channel.is_paused ? '已暂停' : (channel.is_abnormal ? '异常' : '正常')}
        `;
        
        tooltip.style.left = (x + 15) + 'px';
        tooltip.style.top = (y + 15) + 'px';
        tooltip.style.display = 'block';
    }
}

function hideTooltip() {
    const tooltip = document.querySelector('.tooltip');
    if (tooltip) {
        tooltip.style.display = 'none';
    }
}

async function showChannelDetail(cabinetId, channelId) {
    try {
        const response = await fetch(`${API_BASE}/channel/${cabinetId}/${channelId}`);
        const result = await response.json();
        
        if (result.success && result.data) {
            currentChannelData = result.data;
            renderChannelDetail(result.data);
        } else {
            renderMockChannelDetail(cabinetId, channelId);
        }
    } catch (e) {
        console.error('Failed to load channel detail:', e);
        renderMockChannelDetail(cabinetId, channelId);
    }
    
    document.getElementById('channelModal').classList.add('active');
}

function renderMockChannelDetail(cabinetId, channelId) {
    const history = generateMockHistory();
    const trend = generateMockTrend();
    
    currentChannelData = {
        status: {
            cabinet_id: cabinetId,
            channel_id: channelId,
            last_update: new Date().toISOString(),
            current_stage: Math.floor(Math.random() * 5) + 1,
            current_voltage: 3.5 + Math.random() * 0.5,
            current_current: (Math.random() - 0.5) * 3,
            current_temperature: 25 + Math.random() * 10,
            current_capacity: 2.5 + Math.random() * 0.7,
            cycle_index: Math.floor(Math.random() * 5) + 1,
            is_abnormal: Math.random() < 0.1,
            is_paused: Math.random() < 0.05,
            capacity_ratio: 0.85 + Math.random() * 0.15,
            predicted_capacity: 2.8 + Math.random() * 0.4
        },
        history,
        capacity_trend: trend,
        stage_summaries: generateMockStageSummaries(),
        predictions: []
    };
    
    renderChannelDetail(currentChannelData);
}

function generateMockHistory() {
    const points = 100;
    const history = {
        timestamps: [],
        voltages: [],
        currents: [],
        temperatures: [],
        capacities: [],
        stages: []
    };
    
    const now = Date.now();
    for (let i = 0; i < points; i++) {
        const t = i / points;
        history.timestamps.push(new Date(now - (points - i) * 60000).toISOString());
        
        let voltage, current, stage;
        
        if (t < 0.1) {
            stage = 1;
            voltage = 2.8 + t * 10 * 0.2;
            current = 0.1;
        } else if (t < 0.4) {
            stage = 2;
            voltage = 3.0 + (t - 0.1) * 3.33 * 1.2;
            current = 1.5;
        } else if (t < 0.55) {
            stage = 3;
            voltage = 4.15 + Math.random() * 0.05;
            current = 1.5 - (t - 0.4) * 6.67 * 1.4;
        } else if (t < 0.65) {
            stage = 4;
            voltage = 4.0 - (t - 0.55) * 10 * 0.1;
            current = 0;
        } else {
            stage = 5;
            voltage = 3.8 - (t - 0.65) * 2.86 * 1.0;
            current = -1.5;
        }
        
        history.voltages.push(voltage + (Math.random() - 0.5) * 0.02);
        history.currents.push(current + (Math.random() - 0.5) * 0.05);
        history.temperatures.push(28 + t * 8 + Math.sin(t * 10) * 1.5);
        history.capacities.push(Math.max(0, Math.min(3.2, 2.0 + t * 1.2 + (Math.random() - 0.5) * 0.05)));
        history.stages.push(stage);
    }
    
    return history;
}

function generateMockTrend() {
    const cycles = 10;
    return {
        cycle_indices: Array.from({length: cycles}, (_, i) => i + 1),
        charge_capacities: Array.from({length: cycles}, (_, i) => 3.15 - i * 0.01 + (Math.random() - 0.5) * 0.03),
        discharge_capacities: Array.from({length: cycles}, (_, i) => 3.1 - i * 0.015 + (Math.random() - 0.5) * 0.03),
        predicted_capacities: Array.from({length: cycles}, (_, i) => 3.05 - i * 0.01)
    };
}

function generateMockStageSummaries() {
    return [
        { stage: 1, duration: 1800, start_voltage: 2.8, end_voltage: 3.0, avg_current: 0.1, max_temperature: 28, capacity_gain: 0.05 },
        { stage: 2, duration: 7200, start_voltage: 3.0, end_voltage: 4.15, avg_current: 1.5, max_temperature: 35, capacity_gain: 2.8 },
        { stage: 3, duration: 3600, start_voltage: 4.15, end_voltage: 4.2, avg_current: 0.5, max_temperature: 36, capacity_gain: 0.3 },
        { stage: 4, duration: 1800, start_voltage: 4.1, end_voltage: 3.9, avg_current: 0, max_temperature: 32, capacity_gain: 0 },
        { stage: 5, duration: 5400, start_voltage: 3.9, end_voltage: 2.8, avg_current: -1.5, max_temperature: 34, capacity_gain: -2.9 }
    ];
}

function renderChannelDetail(data) {
    const modal = document.getElementById('channelModal');
    const status = data.status;
    
    document.getElementById('modalTitle').textContent = 
        `通道详情 - 化成柜 ${status.cabinet_id} / 通道 ${status.channel_id}`;
    
    const capacityRatio = status.capacity_ratio * 100;
    const ratioClass = capacityRatio >= 95 ? 'good' : (capacityRatio >= 90 ? 'warning' : 'danger');
    
    const infoHtml = `
        <div class="info-item">
            <span class="label">当前阶段</span>
            <span class="value" style="color: ${STAGE_COLORS[status.current_stage]};">${STAGE_NAMES[status.current_stage] || '未知'}</span>
        </div>
        <div class="info-item">
            <span class="label">电压</span>
            <span class="value">${status.current_voltage.toFixed(4)} V</span>
        </div>
        <div class="info-item">
            <span class="label">电流</span>
            <span class="value">${status.current_current.toFixed(4)} A</span>
        </div>
        <div class="info-item">
            <span class="label">温度</span>
            <span class="value ${status.current_temperature > 40 ? 'danger' : ''}">${status.current_temperature.toFixed(1)} °C</span>
        </div>
        <div class="info-item">
            <span class="label">当前容量</span>
            <span class="value">${status.current_capacity.toFixed(4)} Ah</span>
        </div>
        <div class="info-item">
            <span class="label">容量比</span>
            <span class="value ${ratioClass}">${capacityRatio.toFixed(1)}%</span>
        </div>
        <div class="info-item">
            <span class="label">循环次数</span>
            <span class="value">第 ${status.cycle_index} 次</span>
        </div>
        <div class="info-item">
            <span class="label">状态</span>
            <span class="value ${status.is_abnormal ? 'danger' : (status.is_paused ? 'warning' : 'good')}">
                ${status.is_paused ? '已暂停' : (status.is_abnormal ? '异常' : '正常')}
            </span>
        </div>
    `;
    
    document.getElementById('channelInfo').innerHTML = infoHtml;
    
    const pauseBtn = document.getElementById('pauseBtn');
    pauseBtn.textContent = status.is_paused ? '恢复通道' : '暂停通道';
    pauseBtn.classList.toggle('btn-danger', !status.is_paused);
    pauseBtn.dataset.paused = status.is_paused;
    
    renderChargeDischargeChart(data.history);
    renderCapacityTrendChart(data.capacity_trend);
    renderStageSummaries(data.stage_summaries);
    renderPrediction(data);
}

function renderChargeDischargeChart(history) {
    const canvas = document.getElementById('chargeDischargeChart');
    const ctx = canvas.getContext('2d');
    
    const padding = { top: 20, right: 60, bottom: 40, left: 60 };
    const width = canvas.width - padding.left - padding.right;
    const height = canvas.height - padding.top - padding.bottom;
    
    ctx.fillStyle = '#0a0a1a';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    
    if (!history || !history.voltages || history.voltages.length === 0) {
        ctx.fillStyle = '#666';
        ctx.font = '14px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText('暂无历史数据', canvas.width / 2, canvas.height / 2);
        return;
    }
    
    const dataLength = history.voltages.length;
    const xStep = width / (dataLength - 1);
    
    const voltageMin = 2.5;
    const voltageMax = 4.5;
    const currentMin = -2;
    const currentMax = 2;
    const tempMin = 20;
    const tempMax = 50;
    const capacityMin = 0;
    const capacityMax = 3.5;
    
    ctx.strokeStyle = '#222';
    ctx.lineWidth = 1;
    
    for (let i = 0; i <= 5; i++) {
        const y = padding.top + (height / 5) * i;
        ctx.beginPath();
        ctx.moveTo(padding.left, y);
        ctx.lineTo(padding.left + width, y);
        ctx.stroke();
    }
    
    ctx.strokeStyle = '#FF4444';
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    history.voltages.forEach((v, i) => {
        const x = padding.left + i * xStep;
        const y = padding.top + height - ((v - voltageMin) / (voltageMax - voltageMin)) * height;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
    });
    ctx.stroke();
    
    ctx.strokeStyle = '#4444FF';
    ctx.beginPath();
    history.currents.forEach((c, i) => {
        const x = padding.left + i * xStep;
        const y = padding.top + height - ((c - currentMin) / (currentMax - currentMin)) * height;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
    });
    ctx.stroke();
    
    ctx.strokeStyle = '#44AA44';
    ctx.beginPath();
    history.temperatures.forEach((t, i) => {
        const x = padding.left + i * xStep;
        const y = padding.top + height - ((t - tempMin) / (tempMax - tempMin)) * height;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
    });
    ctx.stroke();
    
    ctx.strokeStyle = '#FFAA00';
    ctx.lineWidth = 2;
    ctx.beginPath();
    history.capacities.forEach((cap, i) => {
        const x = padding.left + i * xStep;
        const y = padding.top + height - ((cap - capacityMin) / (capacityMax - capacityMin)) * height;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
    });
    ctx.stroke();
    
    ctx.fillStyle = '#888';
    ctx.font = '11px sans-serif';
    ctx.textAlign = 'right';
    
    for (let i = 0; i <= 5; i++) {
        const y = padding.top + (height / 5) * i;
        const voltage = voltageMax - (voltageMax - voltageMin) * (i / 5);
        ctx.fillText(voltage.toFixed(1), padding.left - 5, y + 4);
    }
    
    ctx.textAlign = 'left';
    for (let i = 0; i <= 5; i++) {
        const y = padding.top + (height / 5) * i;
        const current = currentMax - (currentMax - currentMin) * (i / 5);
        ctx.fillText(current.toFixed(1), padding.left + width + 5, y + 4);
    }
    
    if (history.timestamps && history.timestamps.length > 0) {
        ctx.textAlign = 'center';
        const start = new Date(history.timestamps[0]);
        const end = new Date(history.timestamps[history.timestamps.length - 1]);
        
        for (let i = 0; i <= 4; i++) {
            const x = padding.left + (width / 4) * i;
            const t = new Date(start.getTime() + (end.getTime() - start.getTime()) * (i / 4));
            ctx.fillText(t.toLocaleTimeString('zh-CN', {hour: '2-digit', minute: '2-digit'}), x, padding.top + height + 20);
        }
    }
    
    if (history.stages && history.stages.length > 0) {
        let currentStage = history.stages[0];
        let startX = padding.left;
        
        history.stages.forEach((s, i) => {
            if (s !== currentStage || i === history.stages.length - 1) {
                const endX = padding.left + i * xStep;
                ctx.fillStyle = STAGE_COLORS[currentStage] + '20';
                ctx.fillRect(startX, padding.top, endX - startX, height);
                
                ctx.fillStyle = STAGE_COLORS[currentStage] + '60';
                ctx.fillRect(startX, padding.top, 2, height);
                
                currentStage = s;
                startX = endX;
            }
        });
    }
}

function renderCapacityTrendChart(trend) {
    const canvas = document.getElementById('capacityTrendChart');
    const ctx = canvas.getContext('2d');
    
    const padding = { top: 20, right: 20, bottom: 40, left: 60 };
    const width = canvas.width - padding.left - padding.right;
    const height = canvas.height - padding.top - padding.bottom;
    
    ctx.fillStyle = '#0a0a1a';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    
    if (!trend || !trend.cycle_indices || trend.cycle_indices.length === 0) {
        ctx.fillStyle = '#666';
        ctx.font = '14px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText('暂无趋势数据', canvas.width / 2, canvas.height / 2);
        return;
    }
    
    const cycles = trend.cycle_indices;
    const charge = trend.charge_capacities;
    const discharge = trend.discharge_capacities;
    const predicted = trend.predicted_capacities;
    
    const dataLength = cycles.length;
    const xStep = width / Math.max(dataLength - 1, 1);
    
    const capacityMin = 2.5;
    const capacityMax = 3.5;
    const ratedCapacity = 3.2;
    
    ctx.strokeStyle = '#222';
    ctx.lineWidth = 1;
    
    for (let i = 0; i <= 5; i++) {
        const y = padding.top + (height / 5) * i;
        ctx.beginPath();
        ctx.moveTo(padding.left, y);
        ctx.lineTo(padding.left + width, y);
        ctx.stroke();
    }
    
    const ratedY = padding.top + height - ((ratedCapacity - capacityMin) / (capacityMax - capacityMin)) * height;
    ctx.strokeStyle = '#888';
    ctx.setLineDash([5, 5]);
    ctx.beginPath();
    ctx.moveTo(padding.left, ratedY);
    ctx.lineTo(padding.left + width, ratedY);
    ctx.stroke();
    ctx.setLineDash([]);
    
    ctx.fillStyle = '#888';
    ctx.font = '11px sans-serif';
    ctx.textAlign = 'right';
    for (let i = 0; i <= 5; i++) {
        const y = padding.top + (height / 5) * i;
        const capacity = capacityMax - (capacityMax - capacityMin) * (i / 5);
        ctx.fillText(capacity.toFixed(1), padding.left - 5, y + 4);
    }
    
    ctx.textAlign = 'center';
    const xLabels = Math.min(10, dataLength);
    for (let i = 0; i < xLabels; i++) {
        const idx = Math.floor(i * (dataLength - 1) / Math.max(xLabels - 1, 1));
        const x = padding.left + idx * xStep;
        ctx.fillText(`第${cycles[idx]}次`, x, padding.top + height + 20);
    }
    
    ctx.strokeStyle = '#44AA44';
    ctx.lineWidth = 2;
    ctx.beginPath();
    charge.forEach((c, i) => {
        const x = padding.left + i * xStep;
        const y = padding.top + height - ((c - capacityMin) / (capacityMax - capacityMin)) * height;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
    });
    ctx.stroke();
    
    ctx.strokeStyle = '#FF4444';
    ctx.beginPath();
    discharge.forEach((c, i) => {
        const x = padding.left + i * xStep;
        const y = padding.top + height - ((c - capacityMin) / (capacityMax - capacityMin)) * height;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
    });
    ctx.stroke();
    
    ctx.strokeStyle = '#00d9ff';
    ctx.setLineDash([5, 5]);
    ctx.beginPath();
    predicted.forEach((c, i) => {
        if (c > 0) {
            const x = padding.left + i * xStep;
            const y = padding.top + height - ((c - capacityMin) / (capacityMax - capacityMin)) * height;
            if (i === 0 || predicted[i - 1] === 0) ctx.moveTo(x, y);
            else ctx.lineTo(x, y);
        }
    });
    ctx.stroke();
    ctx.setLineDash([]);
    
    ctx.textAlign = 'left';
    ctx.font = '12px sans-serif';
    ctx.fillStyle = '#44AA44';
    ctx.fillRect(padding.left + 10, 5, 12, 3);
    ctx.fillStyle = '#aaa';
    ctx.fillText('充电容量', padding.left + 28, 10);
    
    ctx.fillStyle = '#FF4444';
    ctx.fillRect(padding.left + 110, 5, 12, 3);
    ctx.fillStyle = '#aaa';
    ctx.fillText('放电容量', padding.left + 128, 10);
    
    ctx.fillStyle = '#00d9ff';
    ctx.fillRect(padding.left + 210, 5, 12, 3);
    ctx.fillStyle = '#aaa';
    ctx.fillText('预测容量', padding.left + 228, 10);
}

function renderStageSummaries(summaries) {
    const container = document.getElementById('stageGrid');
    container.innerHTML = '';
    
    if (!summaries || summaries.length === 0) {
        container.innerHTML = '<div style="color: #666; grid-column: 1/-1; text-align: center;">暂无阶段数据</div>';
        return;
    }
    
    summaries.forEach(summary => {
        const stageName = STAGE_NAMES[summary.stage] || '未知';
        const color = STAGE_COLORS[summary.stage] || '#888';
        
        const card = document.createElement('div');
        card.className = 'stage-card';
        card.style.borderLeftColor = color;
        
        card.innerHTML = `
            <div class="stage-name" style="color: ${color};">${stageName}</div>
            <div class="stage-detail">持续时间: <span>${formatDuration(summary.duration)}</span></div>
            <div class="stage-detail">起始电压: <span>${summary.start_voltage.toFixed(3)} V</span></div>
            <div class="stage-detail">结束电压: <span>${summary.end_voltage.toFixed(3)} V</span></div>
            <div class="stage-detail">平均电流: <span>${summary.avg_current.toFixed(3)} A</span></div>
            <div class="stage-detail">最高温度: <span>${summary.max_temperature.toFixed(1)} °C</span></div>
            <div class="stage-detail">容量变化: <span>${summary.capacity_gain.toFixed(3)} Ah</span></div>
        `;
        
        container.appendChild(card);
    });
}

function renderPrediction(data) {
    const container = document.getElementById('predictionContent');
    const status = data.status;
    const predicted = status.predicted_capacity || 0;
    const rated = 3.2;
    const ratio = predicted / rated;
    const ratioClass = ratio >= 0.95 ? 'good' : (ratio >= 0.90 ? 'warning' : 'danger');
    
    const predictionStatus = status.prediction_status || 0;
    const completedCycles = status.completed_cycles || 0;
    const minCycles = 3;
    
    const statusMap = {
        0: { text: '待预测', class: 'neutral', icon: '⏳' },
        1: { text: '预测中', class: 'warning', icon: '🔄' },
        2: { text: '预测完成', class: 'good', icon: '✅' },
        3: { text: '数据不足', class: 'warning', icon: '⚠️' }
    };
    
    const statusInfo = statusMap[predictionStatus] || statusMap[0];
    const isCompleted = predictionStatus === 2;
    const progressPercent = Math.min(100, (completedCycles / minCycles) * 100);
    
    let predictionHtml = '';
    
    if (isCompleted) {
        predictionHtml = `
            <div class="prediction-item">
                <span class="label">额定容量</span>
                <span class="value">${rated.toFixed(2)} Ah</span>
            </div>
            <div class="prediction-item">
                <span class="label">预测最终容量</span>
                <span class="value ${ratioClass}">${predicted.toFixed(3)} Ah</span>
            </div>
            <div class="prediction-item">
                <span class="label">预测容量比</span>
                <span class="value ${ratioClass}">${(ratio * 100).toFixed(1)}%</span>
            </div>
            <div class="prediction-item">
                <span class="label">当前容量</span>
                <span class="value">${status.current_capacity.toFixed(3)} Ah</span>
            </div>
            <div class="prediction-item">
                <span class="label">预测结果</span>
                <span class="value ${ratio < 0.9 ? 'danger' : 'good'}">
                    ${ratio < 0.9 ? '⚠️ 降级品' : '✅ 合格品'}
                </span>
            </div>
            <div class="prediction-bar">
                <div class="prediction-bar-fill" style="width: ${Math.min(100, ratio * 100)}%;"></div>
            </div>
        `;
    } else {
        let message = '';
        if (predictionStatus === 1) {
            message = `正在收集循环数据，已完成 ${completedCycles}/${minCycles} 个循环`;
        } else if (predictionStatus === 3) {
            message = `数据不完整，已完成 ${completedCycles}/${minCycles} 个循环，需要更多完整循环数据`;
        } else {
            message = `等待循环数据，已完成 ${completedCycles}/${minCycles} 个循环`;
        }
        
        predictionHtml = `
            <div class="prediction-item">
                <span class="label">额定容量</span>
                <span class="value">${rated.toFixed(2)} Ah</span>
            </div>
            <div class="prediction-item">
                <span class="label">预测状态</span>
                <span class="value ${statusInfo.class}">${statusInfo.icon} ${statusInfo.text}</span>
            </div>
            <div class="prediction-item">
                <span class="label">已完成循环</span>
                <span class="value">${completedCycles} / ${minCycles}</span>
            </div>
            <div class="prediction-item">
                <span class="label">当前容量</span>
                <span class="value">${status.current_capacity.toFixed(3)} Ah</span>
            </div>
            <div class="prediction-item" style="grid-column: span 2;">
                <span class="label" style="width: 100%;">${message}</span>
            </div>
            <div class="prediction-bar" style="grid-column: span 2; background: #2a2a3e;">
                <div class="prediction-bar-fill" style="width: ${progressPercent}%; background: linear-gradient(90deg, #667eea, #764ba2);"></div>
            </div>
            <div class="prediction-item" style="grid-column: span 2; text-align: center; color: #888; font-size: 0.9em;">
                完成 ${minCycles} 个完整循环后自动开始容量预测
            </div>
        `;
    }
    
    container.innerHTML = predictionHtml;
}

function formatDuration(seconds) {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    
    if (h > 0) {
        return `${h}小时${m}分钟`;
    } else if (m > 0) {
        return `${m}分钟${s}秒`;
    }
    return `${s}秒`;
}

async function toggleChannelPause() {
    if (!currentChannelData) return;
    
    const status = currentChannelData.status;
    const btn = document.getElementById('pauseBtn');
    const isPaused = btn.dataset.paused === 'true';
    
    try {
        const action = isPaused ? 'resume' : 'pause';
        const response = await fetch(
            `${API_BASE}/channel/${status.cabinet_id}/${status.channel_id}/${action}`,
            { method: 'POST' }
        );
        
        const result = await response.json();
        if (result.success) {
            refreshChannelDetail();
        }
    } catch (e) {
        console.error('Failed to toggle pause:', e);
        refreshChannelDetail();
    }
}

async function predictCapacity() {
    if (!currentChannelData) return;
    
    const status = currentChannelData.status;
    
    try {
        const response = await fetch(`${API_BASE}/predict/${status.cabinet_id}/${status.channel_id}`);
        const result = await response.json();
        
        if (result.success && result.data) {
            currentChannelData.status.predicted_capacity = result.data.predicted_capacity;
            renderChannelDetail(currentChannelData);
        }
    } catch (e) {
        console.error('Failed to predict capacity:', e);
    }
}

async function refreshChannelDetail() {
    if (!currentChannelData) return;
    
    const status = currentChannelData.status;
    await showChannelDetail(status.cabinet_id, status.channel_id);
}

function setupModalEvents() {
    document.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') {
            closeModal();
        }
    });
}

function closeModal() {
    document.getElementById('channelModal').classList.remove('active');
    currentChannelData = null;
}

function refreshPanel() {
    loadCabinetPanel(currentCabinetId);
    loadSystemStats();
    loadAlerts();
}

function toggleAutoRefresh() {
    autoRefresh = !autoRefresh;
    document.getElementById('autoRefreshText').textContent = `自动刷新: ${autoRefresh ? '开' : '关'}`;
}

window.onclick = (e) => {
    const modal = document.getElementById('channelModal');
    if (e.target === modal) {
        closeModal();
    }
};
