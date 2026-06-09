const API_BASE = 'http://localhost:8080/api';

window.MESA = (function() {
    let currentBatchId = null;
    let currentTab = 'params';

    function init() {
        console.log('MES Integration module initialized');
    }

    async function loadBatches() {
        try {
            const tbody = document.getElementById('mesBatchList');
            tbody.innerHTML = '<tr><td colspan="7" class="empty-state"><div class="loading-indicator"></div><div style="margin-top:10px;">加载中...</div></td></tr>';

            const mockBatches = [
                { batch_id: 'BATCH-20240103-001', model: '3.2Ah-18650', cell_count: 512, start_time: '2024-01-03 08:00:00', end_time: '2024-01-03 20:30:00', status: 'completed' },
                { batch_id: 'BATCH-20240103-002', model: '3.2Ah-18650', cell_count: 512, start_time: '2024-01-03 10:00:00', end_time: null, status: 'running' },
                { batch_id: 'BATCH-20240102-001', model: '3.2Ah-21700', cell_count: 512, start_time: '2024-01-02 08:00:00', end_time: '2024-01-02 21:15:00', status: 'completed' },
                { batch_id: 'BATCH-20240102-002', model: '3.2Ah-18650', cell_count: 512, start_time: '2024-01-02 12:00:00', end_time: '2024-01-03 01:30:00', status: 'completed' },
                { batch_id: 'BATCH-20240101-001', model: '3.2Ah-18650', cell_count: 512, start_time: '2024-01-01 08:00:00', end_time: '2024-01-01 19:45:00', status: 'completed' },
            ];

            displayBatches(mockBatches);
        } catch (e) {
            console.error('Failed to load batches:', e);
        }
    }

    function displayBatches(batches) {
        const tbody = document.getElementById('mesBatchList');

        if (!batches || batches.length === 0) {
            tbody.innerHTML = '<tr><td colspan="7" class="empty-state">暂无批次记录</td></tr>';
            return;
        }

        tbody.innerHTML = batches.map(b => {
            const statusClass = `status-${b.status}`;
            const statusText = {
                'running': '进行中',
                'completed': '已完成',
                'paused': '已暂停',
                'error': '异常'
            }[b.status] || b.status;

            return `
                <tr>
                    <td><strong>${b.batch_id}</strong></td>
                    <td>${b.model}</td>
                    <td>${b.cell_count}</td>
                    <td>${b.start_time}</td>
                    <td>${b.end_time || '-'}</td>
                    <td><span class="status-tag ${statusClass}">${statusText}</span></td>
                    <td>
                        <button class="btn btn-sm action-btn" onclick="MESA.viewBatchDetail('${b.batch_id}')">详情</button>
                        <button class="btn btn-sm action-btn" onclick="MESA.exportBatch('${b.batch_id}')">导出</button>
                    </td>
                </tr>
            `;
        }).join('');
    }

    async function searchBatches() {
        const batchId = document.getElementById('mesBatchId').value.trim();
        const startDate = document.getElementById('mesStartDate').value;
        const endDate = document.getElementById('mesEndDate').value;

        console.log('Searching batches:', { batchId, startDate, endDate });
        loadBatches();
    }

    async function viewBatchDetail(batchId) {
        currentBatchId = batchId;
        document.getElementById('mesBatchDetailSection').style.display = 'block';

        loadParamsTab();
        document.getElementById('mesBatchDetailSection').scrollIntoView({ behavior: 'smooth' });
    }

    function showTab(tabName) {
        currentTab = tabName;

        document.querySelectorAll('.tab-btn').forEach(btn => {
            btn.classList.toggle('active', btn.dataset.tab === tabName);
        });

        document.querySelectorAll('.tab-content').forEach(content => {
            content.style.display = 'none';
        });

        document.getElementById(`tab-${tabName}`).style.display = 'block';

        if (tabName === 'params') {
            loadParamsTab();
        } else if (tabName === 'degraded') {
            loadDegradedTab();
        } else if (tabName === 'capacity') {
            loadCapacityTab();
        } else if (tabName === 'sync') {
            loadSyncTab();
        }
    }

    async function loadParamsTab() {
        const tbody = document.querySelector('#paramsTable tbody');
        tbody.innerHTML = '<tr><td colspan="4" class="empty-state"><div class="loading-indicator"></div></td></tr>';

        const mockParams = [
            { timestamp: '2024-01-03 08:00:00', type: '充电电流', value: 1.6, unit: 'A' },
            { timestamp: '2024-01-03 08:00:00', type: '充电电压上限', value: 4.2, unit: 'V' },
            { timestamp: '2024-01-03 08:00:00', type: '放电电流', value: 1.6, unit: 'A' },
            { timestamp: '2024-01-03 08:00:00', type: '放电电压下限', value: 2.75, unit: 'V' },
            { timestamp: '2024-01-03 08:00:00', type: '温度上限', value: 45.0, unit: '°C' },
            { timestamp: '2024-01-03 08:00:00', type: '静置时间', value: 30.0, unit: 'min' },
            { timestamp: '2024-01-03 10:30:00', type: '环境温度', value: 25.5, unit: '°C' },
            { timestamp: '2024-01-03 14:15:00', type: '环境湿度', value: 45, unit: '%' },
        ];

        tbody.innerHTML = mockParams.map(p => `
            <tr>
                <td>${p.timestamp}</td>
                <td>${p.type}</td>
                <td>${p.value}</td>
                <td>${p.unit}</td>
            </tr>
        `).join('');
    }

    async function loadDegradedTab() {
        const tbody = document.querySelector('#degradedTable tbody');
        tbody.innerHTML = '<tr><td colspan="4" class="empty-state"><div class="loading-indicator"></div></td></tr>';

        const mockDegraded = [
            { timestamp: '2024-01-03 12:00:00', channel: '柜0-通道12', reason: '容量偏低', capacity_ratio: 0.88 },
            { timestamp: '2024-01-03 12:15:00', channel: '柜0-通道45', reason: '内阻偏高', capacity_ratio: 0.91 },
            { timestamp: '2024-01-03 13:30:00', channel: '柜0-通道128', reason: '温度异常', capacity_ratio: 0.85 },
            { timestamp: '2024-01-03 14:45:00', channel: '柜0-通道256', reason: '容量偏低', capacity_ratio: 0.87 },
        ];

        tbody.innerHTML = mockDegraded.map(d => {
            const ratioClass = d.capacity_ratio >= 0.9 ? 'warning' : 'danger';
            return `
                <tr>
                    <td>${d.timestamp}</td>
                    <td>${d.channel}</td>
                    <td><span class="badge badge-danger">${d.reason}</span></td>
                    <td><span class="${ratioClass}">${(d.capacity_ratio * 100).toFixed(1)}%</span></td>
                </tr>
            `;
        }).join('');
    }

    async function loadCapacityTab() {
        const canvas = document.getElementById('capacityChart');
        const ctx = canvas.getContext('2d');

        const stats = {
            mean: 3.215,
            std: 0.045,
            median: 3.212,
            skewness: 0.23,
            kurtosis: -0.15,
            min: 3.08,
            max: 3.35,
            p10: 3.16,
            p90: 3.27
        };

        const histogram = generateMockHistogram();
        drawHistogram(ctx, canvas, histogram, stats);

        const statsHtml = `
            <div class="summary-item">
                <span class="label">均值</span>
                <span class="value">${stats.mean.toFixed(3)} Ah</span>
            </div>
            <div class="summary-item">
                <span class="label">标准差</span>
                <span class="value">${stats.std.toFixed(4)} Ah</span>
            </div>
            <div class="summary-item">
                <span class="label">中位数</span>
                <span class="value">${stats.median.toFixed(3)} Ah</span>
            </div>
            <div class="summary-item">
                <span class="label">偏度</span>
                <span class="value ${Math.abs(stats.skewness) > 1 ? 'warning' : 'good'}">${stats.skewness.toFixed(3)}</span>
            </div>
            <div class="summary-item">
                <span class="label">峰度</span>
                <span class="value">${stats.kurtosis.toFixed(3)}</span>
            </div>
            <div class="summary-item">
                <span class="label">范围</span>
                <span class="value">${stats.min.toFixed(2)} - ${stats.max.toFixed(2)} Ah</span>
            </div>
        `;

        document.getElementById('capacityStats').innerHTML = statsHtml;
    }

    function generateMockHistogram() {
        const data = [];
        const bins = 20;
        const start = 3.05;
        const end = 3.35;
        const step = (end - start) / bins;

        for (let i = 0; i < bins; i++) {
            const center = start + step * (i + 0.5);
            const normal = Math.exp(-Math.pow((center - 3.215) / 0.045, 2) / 2);
            const count = Math.round(normal * 60 + Math.random() * 10);
            data.push({ bin: center, count, range: `${(start + step * i).toFixed(3)} - ${(start + step * (i + 1)).toFixed(3)}` });
        }

        return data;
    }

    function drawHistogram(ctx, canvas, data, stats) {
        const width = canvas.width = canvas.parentElement.clientWidth - 40;
        const height = canvas.height = 260;
        const padding = { top: 20, right: 20, bottom: 50, left: 50 };
        const chartWidth = width - padding.left - padding.right;
        const chartHeight = height - padding.top - padding.bottom;

        ctx.fillStyle = '#0a0a1a';
        ctx.fillRect(0, 0, width, height);

        const maxCount = Math.max(...data.map(d => d.count));
        const barWidth = chartWidth / data.length - 2;

        data.forEach((d, i) => {
            const x = padding.left + i * (chartWidth / data.length) + 1;
            const barHeight = (d.count / maxCount) * chartHeight;
            const y = padding.top + chartHeight - barHeight;

            const ratio = (d.bin - 3.05) / (3.35 - 3.05);
            const r = Math.round(255 * Math.max(0, 1 - ratio * 2));
            const g = Math.round(255 * Math.max(0, ratio < 0.5 ? ratio * 2 : 2 - ratio * 2));
            const b = Math.round(255 * Math.max(0, ratio * 2 - 1));

            ctx.fillStyle = `rgb(${r}, ${g}, ${b})`;
            ctx.fillRect(x, y, barWidth, barHeight);

            ctx.fillStyle = '#888';
            ctx.font = '10px sans-serif';
            ctx.textAlign = 'center';
            if (i % 2 === 0) {
                ctx.fillText(d.bin.toFixed(2), x + barWidth / 2, height - 25);
            }
        });

        ctx.strokeStyle = '#333';
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(padding.left, padding.top);
        ctx.lineTo(padding.left, padding.top + chartHeight);
        ctx.lineTo(width - padding.right, padding.top + chartHeight);
        ctx.stroke();

        ctx.fillStyle = '#888';
        ctx.font = '11px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText('容量 (Ah)', width / 2, height - 8);

        ctx.save();
        ctx.translate(12, height / 2);
        ctx.rotate(-Math.PI / 2);
        ctx.fillText('频数', 0, 0);
        ctx.restore();

        const meanX = padding.left + ((stats.mean - 3.05) / (3.35 - 3.05)) * chartWidth;
        ctx.strokeStyle = '#e94560';
        ctx.lineWidth = 2;
        ctx.setLineDash([5, 5]);
        ctx.beginPath();
        ctx.moveTo(meanX, padding.top);
        ctx.lineTo(meanX, padding.top + chartHeight);
        ctx.stroke();
        ctx.setLineDash([]);

        ctx.fillStyle = '#e94560';
        ctx.font = '10px sans-serif';
        ctx.textAlign = 'left';
        ctx.fillText(`均值: ${stats.mean.toFixed(3)}Ah`, meanX + 5, padding.top + 15);
    }

    async function loadSyncTab() {
        const container = document.getElementById('syncStatus');

        const mockStatus = [
            { label: '工艺参数同步', status: 'synced', time: '2024-01-03 20:30:00' },
            { label: '降级电池同步', status: 'synced', time: '2024-01-03 20:31:00' },
            { label: '批次信息同步', status: 'synced', time: '2024-01-03 20:32:00' },
            { label: '容量分布同步', status: 'pending', time: '-' },
        ];

        const statusText = {
            'synced': '已同步',
            'pending': '待同步',
            'failed': '同步失败'
        };

        container.innerHTML = mockStatus.map(s => `
            <div class="sync-status-item">
                <div class="status-label">${s.label}</div>
                <div class="status-value">
                    <span class="status-indicator ${s.status}"></span>
                    <span>${statusText[s.status]}</span>
                </div>
                <div class="status-time">${s.time}</div>
            </div>
        `).join('');
    }

    async function syncParams() {
        if (!currentBatchId) return;
        alert(`正在同步批次 ${currentBatchId} 的工艺参数到MES系统...`);
        setTimeout(() => {
            alert('工艺参数同步成功！');
            loadSyncTab();
        }, 1000);
    }

    async function syncDegraded() {
        if (!currentBatchId) return;
        alert(`正在同步批次 ${currentBatchId} 的降级电池信息到MES系统...`);
        setTimeout(() => {
            alert('降级电池信息同步成功！');
            loadSyncTab();
        }, 1000);
    }

    async function syncBatch() {
        if (!currentBatchId) return;
        alert(`正在同步批次 ${currentBatchId} 的汇总信息到MES系统...`);
        setTimeout(() => {
            alert('批次信息同步成功！');
            loadSyncTab();
        }, 1000);
    }

    function exportBatch(batchId) {
        alert(`导出批次 ${batchId} 数据...\n（实际项目中会生成Excel/PDF文件，包含完整工艺参数、容量分布、异常记录等）`);
    }

    return {
        init,
        loadBatches,
        searchBatches,
        viewBatchDetail,
        showTab,
        syncParams,
        syncDegraded,
        syncBatch,
        exportBatch
    };
})();
