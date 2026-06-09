const API_BASE_V2 = '/api/v2/mes';

window.MESConnector = (function() {
    let currentBatchId = null;
    let connectionStatus = 'disconnected';
    let pendingQueue = [];
    let offlineCache = [];
    let statusPollingTimer = null;

    function init() {
        console.log('MES Connector module initialized');
        startStatusPolling();
        loadOfflineCache();
        updateConnectionStatus();
    }

    async function getStatus() {
        try {
            const response = await fetch(`${API_BASE_V2}/status`, {
                method: 'GET',
                headers: { 'Content-Type': 'application/json' }
            });
            if (response.ok) {
                const data = await response.json();
                connectionStatus = data.connection || 'connected';
                pendingQueue = data.pending_queue || [];
                offlineCache = data.offline_cache || [];
                return data;
            }
        } catch (e) {
            console.error('Failed to get MES status:', e);
            connectionStatus = 'disconnected';
        }
        return {
            connection: connectionStatus,
            pending_queue: pendingQueue,
            offline_cache: offlineCache
        };
    }

    async function loadBatches(params = {}) {
        try {
            const tbody = document.getElementById('mesConnectorBatchList');
            if (tbody) {
                tbody.innerHTML = '<tr><td colspan="8" class="empty-state"><div class="loading-indicator"></div><div style="margin-top:10px;">加载中...</div></td></tr>';
            }

            const queryParams = new URLSearchParams();
            if (params.batch_id) queryParams.append('batch_id', params.batch_id);
            if (params.start_date) queryParams.append('start_date', params.start_date);
            if (params.end_date) queryParams.append('end_date', params.end_date);
            if (params.status) queryParams.append('status', params.status);

            const response = await fetch(`${API_BASE_V2}/batches?${queryParams.toString()}`, {
                method: 'GET',
                headers: { 'Content-Type': 'application/json' }
            });

            let batches = [];
            if (response.ok) {
                const data = await response.json();
                batches = data.batches || data;
            } else {
                batches = generateMockBatches();
            }

            displayBatches(batches);
            return batches;
        } catch (e) {
            console.error('Failed to load batches:', e);
            const mockBatches = generateMockBatches();
            displayBatches(mockBatches);
            return mockBatches;
        }
    }

    function generateMockBatches() {
        return [
            { batch_id: 'BATCH-20240103-001', model: '3.2Ah-18650', cell_count: 512, start_time: '2024-01-03 08:00:00', end_time: '2024-01-03 20:30:00', status: 'completed', sync_status: 'synced' },
            { batch_id: 'BATCH-20240103-002', model: '3.2Ah-18650', cell_count: 512, start_time: '2024-01-03 10:00:00', end_time: null, status: 'running', sync_status: 'syncing' },
            { batch_id: 'BATCH-20240102-001', model: '3.2Ah-21700', cell_count: 512, start_time: '2024-01-02 08:00:00', end_time: '2024-01-02 21:15:00', status: 'completed', sync_status: 'synced' },
            { batch_id: 'BATCH-20240102-002', model: '3.2Ah-18650', cell_count: 512, start_time: '2024-01-02 12:00:00', end_time: '2024-01-03 01:30:00', status: 'completed', sync_status: 'pending' },
            { batch_id: 'BATCH-20240101-001', model: '3.2Ah-18650', cell_count: 512, start_time: '2024-01-01 08:00:00', end_time: '2024-01-01 19:45:00', status: 'completed', sync_status: 'failed' },
        ];
    }

    function displayBatches(batches) {
        const tbody = document.getElementById('mesConnectorBatchList');
        if (!tbody) return;

        if (!batches || batches.length === 0) {
            tbody.innerHTML = '<tr><td colspan="8" class="empty-state">暂无批次记录</td></tr>';
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

            const syncStatusClass = `sync-${b.sync_status}`;
            const syncStatusText = {
                'synced': '已同步',
                'syncing': '同步中',
                'pending': '待同步',
                'failed': '同步失败',
                'offline': '离线缓存'
            }[b.sync_status] || b.sync_status;

            return `
                <tr>
                    <td><strong>${b.batch_id}</strong></td>
                    <td>${b.model}</td>
                    <td>${b.cell_count}</td>
                    <td>${b.start_time}</td>
                    <td>${b.end_time || '-'}</td>
                    <td><span class="status-tag ${statusClass}">${statusText}</span></td>
                    <td><span class="sync-indicator ${syncStatusClass}">${syncStatusText}</span></td>
                    <td>
                        <button class="btn btn-sm action-btn" onclick="MESConnector.viewBatchDetail('${b.batch_id}')">详情</button>
                        <button class="btn btn-sm action-btn sync-btn" onclick="MESConnector.syncBatch('${b.batch_id}')">同步</button>
                    </td>
                </tr>
            `;
        }).join('');
    }

    async function searchBatches() {
        const batchId = document.getElementById('mesConnectorBatchId')?.value.trim();
        const startDate = document.getElementById('mesConnectorStartDate')?.value;
        const endDate = document.getElementById('mesConnectorEndDate')?.value;
        const status = document.getElementById('mesConnectorStatus')?.value;

        console.log('Searching batches via MES Connector:', { batchId, startDate, endDate, status });
        loadBatches({ batch_id: batchId, start_date: startDate, end_date: endDate, status });
    }

    async function viewBatchDetail(batchId) {
        currentBatchId = batchId;
        const detailSection = document.getElementById('mesConnectorBatchDetailSection');
        if (detailSection) {
            detailSection.style.display = 'block';
        }

        try {
            const response = await fetch(`${API_BASE_V2}/batches/${batchId}`, {
                method: 'GET',
                headers: { 'Content-Type': 'application/json' }
            });

            let detail = null;
            if (response.ok) {
                detail = await response.json();
            } else {
                detail = generateMockBatchDetail(batchId);
            }

            displayBatchDetail(detail);
            loadParamsStatus(batchId);
            loadDegradedSync(batchId);
            if (detailSection) {
                detailSection.scrollIntoView({ behavior: 'smooth' });
            }
            return detail;
        } catch (e) {
            console.error('Failed to load batch detail:', e);
            const mockDetail = generateMockBatchDetail(batchId);
            displayBatchDetail(mockDetail);
            loadParamsStatus(batchId);
            loadDegradedSync(batchId);
            return mockDetail;
        }
    }

    function generateMockBatchDetail(batchId) {
        return {
            batch_id: batchId,
            model: '3.2Ah-18650',
            cell_count: 512,
            start_time: '2024-01-03 08:00:00',
            end_time: '2024-01-03 20:30:00',
            status: 'completed',
            operator: '张三',
            shift: '早班',
            equipment: '化成分容柜#01',
            sync_status: 'synced',
            sync_time: '2024-01-03 20:35:00'
        };
    }

    function displayBatchDetail(detail) {
        const container = document.getElementById('mesConnectorBatchDetail');
        if (!container || !detail) return;

        container.innerHTML = `
            <div class="batch-detail-header">
                <h3>批次详情: ${detail.batch_id}</h3>
                <span class="status-tag status-${detail.status}">${{
                    'running': '进行中',
                    'completed': '已完成',
                    'paused': '已暂停',
                    'error': '异常'
                }[detail.status] || detail.status}</span>
            </div>
            <div class="batch-detail-grid">
                <div class="detail-item"><span class="label">型号:</span><span class="value">${detail.model}</span></div>
                <div class="detail-item"><span class="label">电池数量:</span><span class="value">${detail.cell_count}</span></div>
                <div class="detail-item"><span class="label">开始时间:</span><span class="value">${detail.start_time}</span></div>
                <div class="detail-item"><span class="label">结束时间:</span><span class="value">${detail.end_time || '-'}</span></div>
                <div class="detail-item"><span class="label">操作员:</span><span class="value">${detail.operator || '-'}</span></div>
                <div class="detail-item"><span class="label">班次:</span><span class="value">${detail.shift || '-'}</span></div>
                <div class="detail-item"><span class="label">设备:</span><span class="value">${detail.equipment || '-'}</span></div>
                <div class="detail-item"><span class="label">同步状态:</span><span class="value sync-${detail.sync_status}">${{
                    'synced': '已同步',
                    'syncing': '同步中',
                    'pending': '待同步',
                    'failed': '同步失败',
                    'offline': '离线缓存'
                }[detail.sync_status] || detail.sync_status}</span></div>
            </div>
        `;
    }

    async function loadParamsStatus(batchId) {
        const tbody = document.querySelector('#mesConnectorParamsTable tbody');
        if (!tbody) return;

        tbody.innerHTML = '<tr><td colspan="5" class="empty-state"><div class="loading-indicator"></div></td></tr>';

        try {
            const response = await fetch(`${API_BASE_V2}/batches/${batchId}/params/status`, {
                method: 'GET',
                headers: { 'Content-Type': 'application/json' }
            });

            let params = [];
            if (response.ok) {
                const data = await response.json();
                params = data.params || data;
            } else {
                params = generateMockParamsStatus();
            }

            displayParamsStatus(params);
            return params;
        } catch (e) {
            console.error('Failed to load params status:', e);
            const mockParams = generateMockParamsStatus();
            displayParamsStatus(mockParams);
            return mockParams;
        }
    }

    function generateMockParamsStatus() {
        return [
            { timestamp: '2024-01-03 08:00:00', type: '充电电流', value: 1.6, unit: 'A', upload_status: 'synced' },
            { timestamp: '2024-01-03 08:00:00', type: '充电电压上限', value: 4.2, unit: 'V', upload_status: 'synced' },
            { timestamp: '2024-01-03 08:00:00', type: '放电电流', value: 1.6, unit: 'A', upload_status: 'synced' },
            { timestamp: '2024-01-03 08:00:00', type: '放电电压下限', value: 2.75, unit: 'V', upload_status: 'synced' },
            { timestamp: '2024-01-03 08:00:00', type: '温度上限', value: 45.0, unit: '°C', upload_status: 'pending' },
            { timestamp: '2024-01-03 08:00:00', type: '静置时间', value: 30.0, unit: 'min', upload_status: 'pending' },
            { timestamp: '2024-01-03 10:30:00', type: '环境温度', value: 25.5, unit: '°C', upload_status: 'failed' },
            { timestamp: '2024-01-03 14:15:00', type: '环境湿度', value: 45, unit: '%', upload_status: 'offline' },
        ];
    }

    function displayParamsStatus(params) {
        const tbody = document.querySelector('#mesConnectorParamsTable tbody');
        if (!tbody) return;

        tbody.innerHTML = params.map(p => {
            const statusClass = `upload-${p.upload_status}`;
            const statusText = {
                'synced': '已上传',
                'pending': '待上传',
                'failed': '上传失败',
                'offline': '离线缓存'
            }[p.upload_status] || p.upload_status;

            return `
                <tr>
                    <td>${p.timestamp}</td>
                    <td>${p.type}</td>
                    <td>${p.value}</td>
                    <td>${p.unit}</td>
                    <td><span class="upload-status ${statusClass}">${statusText}</span></td>
                </tr>
            `;
        }).join('');
    }

    async function loadDegradedSync(batchId) {
        const tbody = document.querySelector('#mesConnectorDegradedTable tbody');
        if (!tbody) return;

        tbody.innerHTML = '<tr><td colspan="5" class="empty-state"><div class="loading-indicator"></div></td></tr>';

        try {
            const response = await fetch(`${API_BASE_V2}/batches/${batchId}/degraded/status`, {
                method: 'GET',
                headers: { 'Content-Type': 'application/json' }
            });

            let degraded = [];
            if (response.ok) {
                const data = await response.json();
                degraded = data.degraded || data;
            } else {
                degraded = generateMockDegradedSync();
            }

            displayDegradedSync(degraded);
            return degraded;
        } catch (e) {
            console.error('Failed to load degraded sync status:', e);
            const mockDegraded = generateMockDegradedSync();
            displayDegradedSync(mockDegraded);
            return mockDegraded;
        }
    }

    function generateMockDegradedSync() {
        return [
            { timestamp: '2024-01-03 12:00:00', channel: '柜0-通道12', reason: '容量偏低', capacity_ratio: 0.88, sync_status: 'synced' },
            { timestamp: '2024-01-03 12:15:00', channel: '柜0-通道45', reason: '内阻偏高', capacity_ratio: 0.91, sync_status: 'synced' },
            { timestamp: '2024-01-03 13:30:00', channel: '柜0-通道128', reason: '温度异常', capacity_ratio: 0.85, sync_status: 'pending' },
            { timestamp: '2024-01-03 14:45:00', channel: '柜0-通道256', reason: '容量偏低', capacity_ratio: 0.87, sync_status: 'failed' },
        ];
    }

    function displayDegradedSync(degraded) {
        const tbody = document.querySelector('#mesConnectorDegradedTable tbody');
        if (!tbody) return;

        tbody.innerHTML = degraded.map(d => {
            const ratioClass = d.capacity_ratio >= 0.9 ? 'warning' : 'danger';
            const syncStatusClass = `sync-${d.sync_status}`;
            const syncStatusText = {
                'synced': '已同步',
                'pending': '待同步',
                'failed': '同步失败',
                'offline': '离线缓存'
            }[d.sync_status] || d.sync_status;

            return `
                <tr>
                    <td>${d.timestamp}</td>
                    <td>${d.channel}</td>
                    <td><span class="badge badge-danger">${d.reason}</span></td>
                    <td><span class="${ratioClass}">${(d.capacity_ratio * 100).toFixed(1)}%</span></td>
                    <td><span class="sync-indicator ${syncStatusClass}">${syncStatusText}</span></td>
                </tr>
            `;
        }).join('');
    }

    async function syncBatch(batchId) {
        const targetBatchId = batchId || currentBatchId;
        if (!targetBatchId) {
            alert('请先选择一个批次');
            return null;
        }

        try {
            const response = await fetch(`${API_BASE_V2}/batches/${targetBatchId}/sync`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ batch_id: targetBatchId })
            });

            if (response.ok) {
                const result = await response.json();
                alert(`批次 ${targetBatchId} 同步任务已提交，任务ID: ${result.task_id || 'N/A'}`);
                loadBatches();
                if (targetBatchId === currentBatchId) {
                    viewBatchDetail(currentBatchId);
                }
                return result;
            } else {
                throw new Error('Sync request failed');
            }
        } catch (e) {
            console.error('Failed to sync batch:', e);
            alert(`正在同步批次 ${targetBatchId} 到MES系统...\n（离线模式：数据已缓存到本地队列）`);
            addToPendingQueue(targetBatchId, 'batch_sync');
            loadBatches();
            return { status: 'offline', batch_id: targetBatchId };
        }
    }

    async function syncParams(batchId) {
        const targetBatchId = batchId || currentBatchId;
        if (!targetBatchId) return null;

        try {
            const response = await fetch(`${API_BASE_V2}/batches/${targetBatchId}/params/sync`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' }
            });

            if (response.ok) {
                const result = await response.json();
                alert('工艺参数同步成功！');
                loadParamsStatus(targetBatchId);
                return result;
            } else {
                throw new Error('Params sync failed');
            }
        } catch (e) {
            console.error('Failed to sync params:', e);
            alert('工艺参数同步请求已加入离线队列');
            addToPendingQueue(targetBatchId, 'params_sync');
            return { status: 'offline' };
        }
    }

    async function syncDegraded(batchId) {
        const targetBatchId = batchId || currentBatchId;
        if (!targetBatchId) return null;

        try {
            const response = await fetch(`${API_BASE_V2}/batches/${targetBatchId}/degraded/sync`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' }
            });

            if (response.ok) {
                const result = await response.json();
                alert('降级电池信息同步成功！');
                loadDegradedSync(targetBatchId);
                return result;
            } else {
                throw new Error('Degraded sync failed');
            }
        } catch (e) {
            console.error('Failed to sync degraded:', e);
            alert('降级电池同步请求已加入离线队列');
            addToPendingQueue(targetBatchId, 'degraded_sync');
            return { status: 'offline' };
        }
    }

    async function manualSync() {
        try {
            const response = await fetch(`${API_BASE_V2}/sync/manual`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ force: true })
            });

            if (response.ok) {
                const result = await response.json();
                alert(`手动同步已启动，待处理: ${result.pending_count || 0} 项`);
                loadBatches();
                updatePendingQueueDisplay();
                return result;
            } else {
                throw new Error('Manual sync failed');
            }
        } catch (e) {
            console.error('Failed to start manual sync:', e);
            alert('手动同步失败，将在网络恢复后自动重试');
            processOfflineQueue();
            return { status: 'offline' };
        }
    }

    function updateConnectionStatus() {
        const statusEl = document.getElementById('mesConnectorConnectionStatus');
        if (!statusEl) return;

        const statusClass = `conn-${connectionStatus}`;
        const statusText = {
            'connected': '已连接',
            'connecting': '连接中',
            'disconnected': '已断开',
            'reconnecting': '重连中'
        }[connectionStatus] || connectionStatus;

        statusEl.innerHTML = `
            <span class="connection-dot ${statusClass}"></span>
            <span>MES连接: ${statusText}</span>
        `;
    }

    function updatePendingQueueDisplay() {
        const container = document.getElementById('mesConnectorPendingQueue');
        if (!container) return;

        if (pendingQueue.length === 0) {
            container.innerHTML = '<div class="empty-state">待处理队列为空</div>';
            return;
        }

        container.innerHTML = `
            <div class="queue-header">
                <span>待处理队列 (${pendingQueue.length})</span>
                <button class="btn btn-sm" onclick="MESConnector.processOfflineQueue()">立即处理</button>
            </div>
            <div class="queue-list">
                ${pendingQueue.map((item, index) => `
                    <div class="queue-item">
                        <span class="queue-type">${getQueueTypeText(item.type)}</span>
                        <span class="queue-batch">${item.batch_id}</span>
                        <span class="queue-time">${item.created_at}</span>
                        <button class="btn btn-xs" onclick="MESConnector.removeFromQueue(${index})">移除</button>
                    </div>
                `).join('')}
            </div>
        `;
    }

    function getQueueTypeText(type) {
        return {
            'batch_sync': '批次同步',
            'params_sync': '参数同步',
            'degraded_sync': '降级电池同步',
            'capacity_sync': '容量分布同步'
        }[type] || type;
    }

    function updateOfflineCacheDisplay() {
        const container = document.getElementById('mesConnectorOfflineCache');
        if (!container) return;

        const cacheSize = offlineCache.length;
        const cacheStats = getOfflineCacheStats();

        container.innerHTML = `
            <div class="cache-header">
                <span>离线缓存监控</span>
                <span class="cache-count">${cacheSize} 条记录</span>
            </div>
            <div class="cache-stats">
                <div class="cache-stat-item">
                    <span class="label">参数记录:</span>
                    <span class="value">${cacheStats.params || 0}</span>
                </div>
                <div class="cache-stat-item">
                    <span class="label">降级电池:</span>
                    <span class="value">${cacheStats.degraded || 0}</span>
                </div>
                <div class="cache-stat-item">
                    <span class="label">容量数据:</span>
                    <span class="value">${cacheStats.capacity || 0}</span>
                </div>
                <div class="cache-stat-item">
                    <span class="label">最早记录:</span>
                    <span class="value">${cacheStats.earliest || '-'}</span>
                </div>
            </div>
            ${cacheSize > 0 ? `
                <button class="btn btn-sm btn-warning" onclick="MESConnector.clearOfflineCache()">
                    清空离线缓存
                </button>
            ` : ''}
        `;
    }

    function getOfflineCacheStats() {
        const stats = { params: 0, degraded: 0, capacity: 0, earliest: null };
        offlineCache.forEach(item => {
            if (item.type === 'params') stats.params++;
            else if (item.type === 'degraded') stats.degraded++;
            else if (item.type === 'capacity') stats.capacity++;
            if (!stats.earliest || item.created_at < stats.earliest) {
                stats.earliest = item.created_at;
            }
        });
        return stats;
    }

    function addToPendingQueue(batchId, type) {
        const item = {
            batch_id: batchId,
            type: type,
            created_at: new Date().toISOString().replace('T', ' ').substring(0, 19),
            retries: 0
        };
        pendingQueue.push(item);
        updatePendingQueueDisplay();
        saveOfflineCache();
    }

    function removeFromQueue(index) {
        if (index >= 0 && index < pendingQueue.length) {
            pendingQueue.splice(index, 1);
            updatePendingQueueDisplay();
            saveOfflineCache();
        }
    }

    async function processOfflineQueue() {
        if (pendingQueue.length === 0) {
            alert('待处理队列为空');
            return;
        }

        if (connectionStatus !== 'connected') {
            alert('当前未连接到MES，无法处理离线队列');
            return;
        }

        const queueCopy = [...pendingQueue];
        pendingQueue = [];

        for (const item of queueCopy) {
            try {
                if (item.type === 'batch_sync') {
                    await syncBatch(item.batch_id);
                } else if (item.type === 'params_sync') {
                    await syncParams(item.batch_id);
                } else if (item.type === 'degraded_sync') {
                    await syncDegraded(item.batch_id);
                }
            } catch (e) {
                console.error('Failed to process queue item:', item, e);
                item.retries = (item.retries || 0) + 1;
                if (item.retries < 3) {
                    pendingQueue.push(item);
                }
            }
        }

        updatePendingQueueDisplay();
        saveOfflineCache();
        alert(`队列处理完成，剩余 ${pendingQueue.length} 项待重试`);
    }

    function loadOfflineCache() {
        try {
            const stored = localStorage.getItem('mes_connector_cache');
            if (stored) {
                const data = JSON.parse(stored);
                pendingQueue = data.pending_queue || [];
                offlineCache = data.offline_cache || [];
            }
        } catch (e) {
            console.error('Failed to load offline cache:', e);
        }
        updatePendingQueueDisplay();
        updateOfflineCacheDisplay();
    }

    function saveOfflineCache() {
        try {
            localStorage.setItem('mes_connector_cache', JSON.stringify({
                pending_queue: pendingQueue,
                offline_cache: offlineCache
            }));
        } catch (e) {
            console.error('Failed to save offline cache:', e);
        }
    }

    function clearOfflineCache() {
        if (confirm('确定要清空所有离线缓存吗？此操作不可恢复。')) {
            offlineCache = [];
            pendingQueue = [];
            localStorage.removeItem('mes_connector_cache');
            updatePendingQueueDisplay();
            updateOfflineCacheDisplay();
        }
    }

    function startStatusPolling() {
        if (statusPollingTimer) {
            clearInterval(statusPollingTimer);
        }
        statusPollingTimer = setInterval(async () => {
            await getStatus();
            updateConnectionStatus();
            updatePendingQueueDisplay();
            updateOfflineCacheDisplay();
        }, 5000);
    }

    function stopStatusPolling() {
        if (statusPollingTimer) {
            clearInterval(statusPollingTimer);
            statusPollingTimer = null;
        }
    }

    return {
        init,
        loadBatches,
        syncBatch,
        getStatus,
        displayBatches,
        searchBatches,
        viewBatchDetail,
        syncParams,
        syncDegraded,
        manualSync,
        processOfflineQueue,
        removeFromQueue,
        clearOfflineCache,
        startStatusPolling,
        stopStatusPolling
    };
})();
