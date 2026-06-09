const API_BASE = 'http://localhost:8080/api';

window.CellGrouping = (function() {
    let currentBatchId = null;

    function init() {
        console.log('Cell Grouping module initialized');
    }

    async function runGrouping() {
        const batchId = document.getElementById('groupingBatchId').value.trim();
        const cellsPerGroup = parseInt(document.getElementById('cellsPerGroup').value);
        const algorithm = document.getElementById('groupingAlgorithm').value;
        const capacityThreshold = parseFloat(document.getElementById('capacityThreshold').value);
        const resistanceThreshold = parseFloat(document.getElementById('resistanceThreshold').value);

        if (!batchId) {
            alert('请输入批次号');
            return;
        }

        const resultSection = document.getElementById('groupingResultSection');
        resultSection.style.display = 'block';
        document.getElementById('groupingSummary').innerHTML = '<div class="empty-state"><div class="loading-indicator"></div><div style="margin-top:10px;">正在计算最优配组方案...</div></div>';

        try {
            const response = await fetch(`${API_BASE}/grouping`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    batch_id: batchId,
                    cells_per_group: cellsPerGroup,
                    algorithm,
                    capacity_threshold: capacityThreshold,
                    resistance_threshold: resistanceThreshold,
                })
            });

            const result = await response.json();

            if (result.success && result.data) {
                displayGroupingResult(result.data);
            } else {
                throw new Error(result.message || '配组计算失败');
            }
        } catch (e) {
            console.error('Grouping error:', e);
            document.getElementById('groupingSummary').innerHTML = `
                <div class="empty-state">
                    <div class="empty-state-icon">⚠️</div>
                    <div>配组计算失败</div>
                    <div style="color:#888; font-size:12px; margin-top:5px;">${e.message}</div>
                </div>
            `;
        }
    }

    function displayGroupingResult(data) {
        const summaryHtml = `
            <div class="summary-item">
                <span class="label">电池总数</span>
                <span class="value">${data.total_cells}</span>
            </div>
            <div class="summary-item">
                <span class="label">配组数量</span>
                <span class="value">${data.group_count}</span>
            </div>
            <div class="summary-item">
                <span class="label">降级电池</span>
                <span class="value ${data.rejected_cells > 0 ? 'danger' : 'good'}">${data.rejected_cells}</span>
            </div>
            <div class="summary-item">
                <span class="label">平均一致性</span>
                <span class="value ${data.avg_consistency_score > 90 ? 'good' : data.avg_consistency_score > 75 ? 'warning' : 'danger'}">${data.avg_consistency_score.toFixed(1)}%</span>
            </div>
            <div class="summary-item">
                <span class="label">计算耗时</span>
                <span class="value">${data.processing_time_ms}ms</span>
            </div>
            <div class="summary-item">
                <span class="label">使用算法</span>
                <span class="value">${data.algorithm === 'genetic' ? '遗传算法' : '贪心算法'}</span>
            </div>
        `;

        document.getElementById('groupingSummary').innerHTML = summaryHtml;

        const groupsHtml = data.groups.map(group => {
            const scoreClass = group.consistency_score > 90 ? 'excellent' : group.consistency_score > 75 ? 'good' : 'fair';
            const cellsHtml = group.cell_cabinet_ids.map((cab, i) => `
                <div class="group-cell-item">
                    <span class="cell-id">柜${cab}通道${group.cell_channel_ids[i]}</span>
                    <span class="cell-specs">${(group.cell_capacities[i] * 100).toFixed(1)}% | ${group.cell_resistances[i] ? group.cell_resistances[i].toFixed(2) : 'N/A'}mΩ</span>
                </div>
            `).join('');

            return `
                <div class="group-card">
                    <div class="group-card-header">
                        <span class="group-card-title">第 ${group.group_number} 组</span>
                        <span class="group-score ${scoreClass}">${group.consistency_score.toFixed(1)}</span>
                    </div>
                    <div class="group-card-stats">
                        <div class="stat"><span>平均容量</span><span>${(group.avg_capacity * 100).toFixed(2)}%</span></div>
                        <div class="stat"><span>容量标准差</span><span>${group.capacity_std.toFixed(4)}</span></div>
                        <div class="stat"><span>容量最大差</span><span>${(group.capacity_max_diff * 100).toFixed(2)}%</span></div>
                        <div class="stat"><span>平均内阻</span><span>${group.avg_resistance.toFixed(3)}mΩ</span></div>
                        <div class="stat"><span>内阻标准差</span><span>${group.resistance_std.toFixed(4)}</span></div>
                        <div class="stat"><span>内阻最大差</span><span>${group.resistance_max_diff.toFixed(3)}mΩ</span></div>
                    </div>
                    <div class="group-cells">${cellsHtml}</div>
                </div>
            `;
        }).join('');

        document.getElementById('groupsContainer').innerHTML = groupsHtml;
        document.getElementById('groupingResultSection').scrollIntoView({ behavior: 'smooth' });
    }

    async function loadGroupingList() {
        try {
            const response = await fetch(`${API_BASE}/grouping/list?limit=20`);
            const result = await response.json();

            if (result.success && result.data) {
                displayGroupingList(result.data);
            }
        } catch (e) {
            console.error('Failed to load grouping list:', e);
            document.getElementById('groupingList').innerHTML = `
                <tr><td colspan="6" class="empty-state">加载失败</td></tr>
            `;
        }
    }

    function displayGroupingList(results) {
        const tbody = document.getElementById('groupingList');

        if (!results || results.length === 0) {
            tbody.innerHTML = '<tr><td colspan="6" class="empty-state">暂无配组记录</td></tr>';
            return;
        }

        tbody.innerHTML = results.map(r => {
            const scoreClass = r.avg_consistency_score > 90 ? 'good' : r.avg_consistency_score > 75 ? 'warning' : 'danger';
            return `
                <tr>
                    <td><strong>${r.batch_id}</strong></td>
                    <td>${r.algorithm === 'genetic' ? '遗传算法' : '贪心算法'}</td>
                    <td>${r.total_cells}</td>
                    <td>${r.group_count}</td>
                    <td><span class="${scoreClass}">${r.avg_consistency_score.toFixed(1)}%</span></td>
                    <td>
                        <button class="btn btn-sm action-btn" onclick="viewGroupingResult('${r.batch_id}')">查看详情</button>
                        <button class="btn btn-sm action-btn" onclick="exportGrouping('${r.batch_id}')">导出</button>
                    </td>
                </tr>
            `;
        }).join('');
    }

    async function viewGroupingResult(batchId) {
        document.getElementById('groupingBatchId').value = batchId;

        try {
            const response = await fetch(`${API_BASE}/grouping/${batchId}`);
            const result = await response.json();

            if (result.success && result.data) {
                document.getElementById('groupingResultSection').style.display = 'block';
                displayGroupingResult(result.data);
            }
        } catch (e) {
            console.error('Failed to load grouping result:', e);
        }
    }

    function exportGrouping(batchId) {
        alert(`导出配组方案：${batchId}\n（实际项目中会生成Excel/PDF文件）`);
    }

    return {
        init,
        runGrouping,
        loadGroupingList,
        viewGroupingResult,
        exportGrouping
    };
})();

function viewGroupingResult(batchId) {
    window.CellGrouping.viewGroupingResult(batchId);
}

function exportGrouping(batchId) {
    window.CellGrouping.exportGrouping(batchId);
}
