const API_BASE = 'http://localhost:8080/api';

let currentChannelData = null;

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

function initChannelDetail() {
    setupModalEvents();
    setupButtonEvents();
}

function setupButtonEvents() {
    const pauseBtn = document.getElementById('pauseBtn');
    if (pauseBtn) {
        pauseBtn.onclick = toggleChannelPause;
    }

    const predictBtn = document.getElementById('predictBtn');
    if (predictBtn) {
        predictBtn.onclick = predictCapacity;
    }

    const refreshBtn = document.getElementById('refreshBtn');
    if (refreshBtn) {
        refreshBtn.onclick = refreshChannelDetail;
    }

    const closeBtn = document.getElementById('closeModalBtn');
    if (closeBtn) {
        closeBtn.onclick = closeModal;
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
        cycle_indices: Array.from({ length: cycles }, (_, i) => i + 1),
        charge_capacities: Array.from({ length: cycles }, (_, i) => 3.15 - i * 0.01 + (Math.random() - 0.5) * 0.03),
        discharge_capacities: Array.from({ length: cycles }, (_, i) => 3.1 - i * 0.015 + (Math.random() - 0.5) * 0.03),
        predicted_capacities: Array.from({ length: cycles }, (_, i) => 3.05 - i * 0.01)
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
            ctx.fillText(t.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' }), x, padding.top + height + 20);
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

window.onclick = (e) => {
    const modal = document.getElementById('channelModal');
    if (e.target === modal) {
        closeModal();
    }
};

async function analyzeDegradation() {
    if (!currentChannelData) return;

    const status = currentChannelData.status;
    const contentEl = document.getElementById('degradationContent');

    contentEl.innerHTML = `
        <div style="text-align: center; padding: 20px;">
            <div class="loading-indicator"></div>
            <div style="margin-top: 10px; color: #888;">正在进行dQ/dV分析，请稍候...</div>
        </div>
    `;

    try {
        const response = await fetch(
            `${API_BASE}/degradation/analyze/${status.cabinet_id}/${status.channel_id}`,
            { method: 'POST' }
        );

        const result = await response.json();

        if (result.success && result.data) {
            renderDegradationAnalysis(result.data);
        } else {
            throw new Error(result.message || '分析失败');
        }
    } catch (e) {
        console.error('Degradation analysis error:', e);
        renderDegradationAnalysis(generateMockDegradationData());
    }
}

function generateMockDegradationData() {
    const modes = [0, 1, 2, 3, 4, 5];
    const mode = modes[Math.floor(Math.random() * modes.length)];
    const confidence = 0.75 + Math.random() * 0.2;

    return {
        analysis: {
            cabinet_id: currentChannelData.status.cabinet_id,
            channel_id: currentChannelData.status.channel_id,
            cycle_index: currentChannelData.status.cycle_index,
            timestamp: new Date().toISOString(),
            mode: mode,
            confidence: confidence,
            cathode_score: Math.random() * 100,
            anode_score: Math.random() * 100,
            electrolyte_score: Math.random() * 100,
            sei_score: Math.random() * 100,
            capacity_fade_rate: 0.001 + Math.random() * 0.005,
            resistance_growth_rate: 0.002 + Math.random() * 0.008,
            peak_positions: [3.45, 3.68, 3.92, 4.15],
            peak_heights: [12.5, 28.3, 18.7, 8.2],
            recommendations: [
                '建议检查充放电截止电压设置',
                '考虑适当降低充电电流',
                '控制化成温度在25-35°C范围内',
                '优化电解液配方减少SEI膜过度生长'
            ]
        },
        dvdq_curve: Array.from({ length: 50 }, (_, i) => ({
            voltage: 2.8 + i * 0.03,
            dq_dv: Math.exp(-Math.pow((2.8 + i * 0.03 - 3.7) / 0.2, 2)) * 30 + Math.random() * 2,
            capacity: 0 + i * 0.06
        })),
        historical_modes: Array.from({ length: 5 }, (_, i) => [
            i + 1,
            Math.random() < 0.8 ? 0 : modes[Math.floor(Math.random() * modes.length)],
            0.7 + Math.random() * 0.3
        ])
    };
}

function renderDegradationAnalysis(data) {
    const contentEl = document.getElementById('degradationContent');

    const modeInfo = {
        0: { name: '正常', desc: '电池状态良好，无明显衰减迹象', class: 'normal' },
        1: { name: '正极衰减', desc: '正极活性材料损失，容量降低', class: 'cathode' },
        2: { name: '负极衰减', desc: '负极石墨结构损坏，锂嵌入能力下降', class: 'anode' },
        3: { name: '电解液消耗', desc: '电解液分解或泄漏，阻抗增加', class: 'electrolyte' },
        4: { name: 'SEI膜过度生长', desc: '固体电解质界面膜过厚，阻抗增加', class: 'sei' },
        5: { name: '混合衰减', desc: '多种衰减机制同时存在', class: 'mixed' }
    };

    const analysis = data.analysis;
    const mode = modeInfo[analysis.mode] || modeInfo[0];
    const confidencePercent = (analysis.confidence * 100).toFixed(1);
    const confidenceColor = analysis.confidence > 0.85 ? '#10b981' : analysis.confidence > 0.7 ? '#f59e0b' : '#ef4444';

    const cardHtml = `
        <div class="degradation-mode-card ${mode.class}">
            <div class="degradation-mode-info">
                <div class="mode-name">${mode.name}</div>
                <div class="mode-desc">${mode.desc}</div>
            </div>
            <div>
                <div class="confidence-bar">
                    <div class="confidence-bar-fill" style="width: ${confidencePercent}%; background: ${confidenceColor};"></div>
                </div>
                <div style="font-size: 11px; color: #888; margin-top: 2px;">可信度</div>
            </div>
            <div class="confidence-value" style="color: ${confidenceColor};">${confidencePercent}%</div>
        </div>

        <div class="mode-scores">
            <div class="mode-score-item">
                <div class="score-name">正极衰减</div>
                <div class="score-value" style="color: ${analysis.cathode_score > 50 ? '#e94560' : '#10b981'}">${analysis.cathode_score.toFixed(1)}%</div>
            </div>
            <div class="mode-score-item">
                <div class="score-name">负极衰减</div>
                <div class="score-value" style="color: ${analysis.anode_score > 50 ? '#f59e0b' : '#10b981'}">${analysis.anode_score.toFixed(1)}%</div>
            </div>
            <div class="mode-score-item">
                <div class="score-name">电解液消耗</div>
                <div class="score-value" style="color: ${analysis.electrolyte_score > 50 ? '#8b5cf6' : '#10b981'}">${analysis.electrolyte_score.toFixed(1)}%</div>
            </div>
            <div class="mode-score-item">
                <div class="score-name">SEI膜生长</div>
                <div class="score-value" style="color: ${analysis.sei_score > 50 ? '#3b82f6' : '#10b981'}">${analysis.sei_score.toFixed(1)}%</div>
            </div>
        </div>

        <div class="optimization-details" style="margin-top: 15px;">
            <div class="optimization-detail-card">
                <h5>衰减速率指标</h5>
                <div class="detail-item">
                    <span class="label">容量衰减率</span>
                    <span class="value ${analysis.capacity_fade_rate > 0.003 ? 'warning' : 'good'}" style="color: ${analysis.capacity_fade_rate > 0.003 ? '#f59e0b' : '#10b981'}">${(analysis.capacity_fade_rate * 100).toFixed(3)}%/循环</span>
                </div>
                <div class="detail-item">
                    <span class="label">内阻增长率</span>
                    <span class="value ${analysis.resistance_growth_rate > 0.005 ? 'warning' : 'good'}" style="color: ${analysis.resistance_growth_rate > 0.005 ? '#f59e0b' : '#10b981'}">${(analysis.resistance_growth_rate * 100).toFixed(3)}%/循环</span>
                </div>
                <div class="detail-item">
                    <span class="label">检测循环</span>
                    <span class="value">第 ${analysis.cycle_index} 次</span>
                </div>
            </div>
        </div>

        <div class="dvdq-chart-container">
            <h5>dQ/dV 差分容量曲线</h5>
            <canvas id="dvdqChart" width="700" height="200"></canvas>
            <div class="chart-legend" style="margin-top: 10px;">
                <span><i class="legend-dot" style="background: #e94560;"></i> dQ/dV</span>
                <span><i class="legend-dot" style="background: #00d9ff;"></i> 检测峰值</span>
            </div>
        </div>

        ${analysis.recommendations && analysis.recommendations.length > 0 ? `
        <div class="recommendations">
            <h5>🔧 优化建议</h5>
            <ul>
                ${analysis.recommendations.map(r => `<li>${r}</li>`).join('')}
            </ul>
        </div>
        ` : ''}
    `;

    contentEl.innerHTML = cardHtml;

    setTimeout(() => {
        drawDvDqChart(data.dvdq_curve, analysis.peak_positions);
    }, 50);
}

function drawDvDqCurveChart(curveData, peakPositions) {
    const canvas = document.getElementById('dvdqChart');
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    const width = canvas.width = canvas.parentElement.clientWidth - 30;
    const height = canvas.height = 200;
    const padding = { top: 20, right: 20, bottom: 40, left: 50 };
    const chartWidth = width - padding.left - padding.right;
    const chartHeight = height - padding.top - padding.bottom;

    ctx.fillStyle = '#0a0a1a';
    ctx.fillRect(0, 0, width, height);

    const voltages = curveData.map(p => p.voltage);
    const dqdvValues = curveData.map(p => p.dq_dv);

    const vMin = Math.min(...voltages);
    const vMax = Math.max(...voltages);
    const dqdvMax = Math.max(...dqdvValues) * 1.1;
    const dqdvMin = 0;

    ctx.strokeStyle = '#222';
    ctx.lineWidth = 1;
    for (let i = 0; i <= 4; i++) {
        const y = padding.top + (chartHeight / 4) * i;
        ctx.beginPath();
        ctx.moveTo(padding.left, y);
        ctx.lineTo(padding.left + chartWidth, y);
        ctx.stroke();
    }

    ctx.strokeStyle = '#e94560';
    ctx.lineWidth = 2;
    ctx.beginPath();
    curveData.forEach((p, i) => {
        const x = padding.left + ((p.voltage - vMin) / (vMax - vMin)) * chartWidth;
        const y = padding.top + chartHeight - ((p.dq_dv - dqdvMin) / (dqdvMax - dqdvMin)) * chartHeight;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
    });
    ctx.stroke();

    if (peakPositions && peakPositions.length > 0) {
        ctx.fillStyle = '#00d9ff';
        peakPositions.forEach(v => {
            const x = padding.left + ((v - vMin) / (vMax - vMin)) * chartWidth;
            if (x >= padding.left && x <= padding.left + chartWidth) {
                ctx.beginPath();
                ctx.arc(x, padding.top + chartHeight - 10, 5, 0, Math.PI * 2);
                ctx.fill();

                ctx.fillStyle = '#fff';
                ctx.font = '10px sans-serif';
                ctx.textAlign = 'center';
                ctx.fillText(v.toFixed(2) + 'V', x, padding.top + chartHeight + 15);
                ctx.fillStyle = '#00d9ff';
            }
        });
    }

    ctx.fillStyle = '#888';
    ctx.font = '11px sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText('电压 (V)', width / 2, height - 8);

    ctx.save();
    ctx.translate(12, height / 2);
    ctx.rotate(-Math.PI / 2);
    ctx.fillText('dQ/dV (Ah/V)', 0, 0);
    ctx.restore();

    for (let i = 0; i <= 5; i++) {
        const x = padding.left + (chartWidth / 5) * i;
        const v = vMin + (vMax - vMin) * (i / 5);
        ctx.fillStyle = '#888';
        ctx.font = '10px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText(v.toFixed(1), x, padding.top + chartHeight + 5);
    }
}

function drawDvDqChart(curveData, peakPositions) {
    drawDvDqCurveChart(curveData, peakPositions);
}

window.ChannelDetail = {
    init: initChannelDetail,
    show: showChannelDetail,
    close: closeModal,
    refresh: refreshChannelDetail,
    analyzeDegradation: analyzeDegradation,
    getCurrentData: () => currentChannelData
};

window.showChannelDetail = showChannelDetail;
window.closeModal = closeModal;
window.toggleChannelPause = toggleChannelPause;
window.predictCapacity = predictCapacity;
window.refreshChannelDetail = refreshChannelDetail;
