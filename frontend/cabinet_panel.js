const API_BASE = 'http://localhost:8080/api';

let currentCabinetId = 0;
let autoRefresh = true;
let refreshInterval = null;
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

function initCabinetPanel() {
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
}

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

    canvas.addEventListener('click', (e) => {
        const rect = canvas.getBoundingClientRect();
        const scaleX = canvas.width / rect.width;
        const scaleY = canvas.height / rect.height;

        const x = (e.clientX - rect.left) * scaleX;
        const y = (e.clientY - rect.top) * scaleY;

        const channelId = getChannelFromPosition(x, y);
        if (channelId !== null && typeof window.showChannelDetail === 'function') {
            window.showChannelDetail(currentCabinetId, channelId);
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

function refreshPanel() {
    loadCabinetPanel(currentCabinetId);
    loadSystemStats();
    loadAlerts();
}

function toggleAutoRefresh() {
    autoRefresh = !autoRefresh;
    document.getElementById('autoRefreshText').textContent = `自动刷新: ${autoRefresh ? '开' : '关'}`;
}

window.CabinetPanel = {
    init: initCabinetPanel,
    refresh: refreshPanel,
    toggleAutoRefresh: toggleAutoRefresh,
    getChannelCache: () => channelDataCache,
    getCurrentCabinet: () => currentCabinetId,
    getStageNames: () => STAGE_NAMES,
    getStageColors: () => STAGE_COLORS
};
