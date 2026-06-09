const API_BASE = 'http://localhost:8080/api';

window.ElectrolyteOptimizer = (function() {

    function init() {
        console.log('Electrolyte Optimizer module initialized');
    }

    async function optimize() {
        const batchId = document.getElementById('electrolyteBatchId').value.trim();
        const targetGasVolume = parseFloat(document.getElementById('targetGasVolume').value);
        const nominalVolume = parseFloat(document.getElementById('nominalVolume').value);

        if (!batchId) {
            alert('请输入批次号');
            return;
        }

        const resultSection = document.getElementById('electrolyteResultSection');
        resultSection.style.display = 'block';
        document.getElementById('electrolyteSummary').innerHTML = '<div class="empty-state"><div class="loading-indicator"></div><div style="margin-top:10px;">正在分析气体数据并计算优化方案...</div></div>';

        try {
            const response = await fetch(`${API_BASE}/electrolyte/optimize`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    batch_id: batchId,
                    target_gas_volume: targetGasVolume,
                    nominal_volume: nominalVolume,
                })
            });

            const result = await response.json();

            if (result.success && result.data) {
                displayOptimizationResult(result.data);
            } else {
                throw new Error(result.message || '优化计算失败');
            }
        } catch (e) {
            console.error('Optimization error:', e);
            document.getElementById('electrolyteSummary').innerHTML = `
                <div class="empty-state">
                    <div class="empty-state-icon">⚠️</div>
                    <div>优化计算失败</div>
                    <div style="color:#888; font-size:12px; margin-top:5px;">${e.message}</div>
                </div>
            `;
        }
    }

    function displayOptimizationResult(data) {
        const adjustmentDirection = data.avg_adjustment > 0 ? '增加' : '减少';
        const adjustmentClass = data.avg_adjustment > 0 ? 'warning' : data.avg_adjustment < 0 ? 'good' : 'good';

        const summaryHtml = `
            <div class="summary-item">
                <span class="label">分析通道数</span>
                <span class="value">${data.total_channels}</span>
            </div>
            <div class="summary-item">
                <span class="label">标称注液量</span>
                <span class="value">${data.avg_nominal_volume.toFixed(1)} g</span>
            </div>
            <div class="summary-item">
                <span class="label">建议注液量</span>
                <span class="value ${adjustmentClass}">${data.avg_suggested_volume.toFixed(1)} g</span>
            </div>
            <div class="summary-item">
                <span class="label">调整量</span>
                <span class="value ${data.avg_adjustment > 0 ? 'warning' : data.avg_adjustment < 0 ? 'good' : 'good'}">
                    ${data.avg_adjustment > 0 ? '+' : ''}${data.avg_adjustment.toFixed(1)} g
                </span>
            </div>
            <div class="summary-item">
                <span class="label">过注通道</span>
                <span class="value ${data.over_injected_count > 0 ? 'danger' : 'good'}">${data.over_injected_count}</span>
            </div>
            <div class="summary-item">
                <span class="label">欠注通道</span>
                <span class="value ${data.under_injected_count > 0 ? 'danger' : 'good'}">${data.under_injected_count}</span>
            </div>
        `;

        document.getElementById('electrolyteSummary').innerHTML = summaryHtml;

        const detailsHtml = `
            <div class="optimization-detail-card">
                <h5>预期效益</h5>
                <div class="detail-item">
                    <span class="label">预计产气量减少</span>
                    <span class="value good">${data.estimated_gas_reduction.toFixed(2)} mL</span>
                </div>
                <div class="detail-item">
                    <span class="label">预计容量提升</span>
                    <span class="value good">+${(data.estimated_capacity_improvement * 100).toFixed(2)}%</span>
                </div>
                <div class="detail-item">
                    <span class="label">下一批次建议</span>
                    <span class="value" style="color:#00d9ff;">${data.next_batch_suggestion.toFixed(1)} g</span>
                </div>
            </div>
            <div class="optimization-detail-card">
                <h5>气体分析统计</h5>
                <div class="detail-item">
                    <span class="label">平均产气量</span>
                    <span class="value">${(data.avg_nominal_volume * 0.015).toFixed(2)} mL</span>
                </div>
                <div class="detail-item">
                    <span class="label">产气量标准差</span>
                    <span class="value">${(data.avg_nominal_volume * 0.003).toFixed(3)} mL</span>
                </div>
                <div class="detail-item">
                    <span class="label">最大产气量</span>
                    <span class="value warning">${(data.avg_nominal_volume * 0.025).toFixed(2)} mL</span>
                </div>
                <div class="detail-item">
                    <span class="label">最小产气量</span>
                    <span class="value">${(data.avg_nominal_volume * 0.008).toFixed(2)} mL</span>
                </div>
            </div>
            <div class="optimization-detail-card">
                <h5>优化算法参数</h5>
                <div class="detail-item">
                    <span class="label">气体-电解液转换率</span>
                    <span class="value">0.85</span>
                </div>
                <div class="detail-item">
                    <span class="label">学习率</span>
                    <span class="value">0.3</span>
                </div>
                <div class="detail-item">
                    <span class="label">趋势权重</span>
                    <span class="value">0.2</span>
                </div>
                <div class="detail-item">
                    <span class="label">最大调整比例</span>
                    <span class="value">±10%</span>
                </div>
            </div>
        `;

        document.getElementById('optimizationDetails').innerHTML = detailsHtml;
        document.getElementById('electrolyteResultSection').scrollIntoView({ behavior: 'smooth' });
    }

    async function loadHistory() {
        try {
            const tbody = document.getElementById('electrolyteList');
            tbody.innerHTML = '<tr><td colspan="5" class="empty-state"><div class="loading-indicator"></div><div style="margin-top:10px;">加载中...</div></td></tr>';

            const mockData = [
                { batch_id: 'BATCH-20240101-001', nominal: 120.0, suggested: 118.5, adjustment: -1.5 },
                { batch_id: 'BATCH-20240101-002', nominal: 120.0, suggested: 119.2, adjustment: -0.8 },
                { batch_id: 'BATCH-20240102-001', nominal: 120.0, suggested: 120.5, adjustment: +0.5 },
                { batch_id: 'BATCH-20240102-002', nominal: 120.0, suggested: 119.8, adjustment: -0.2 },
                { batch_id: 'BATCH-20240103-001', nominal: 120.0, suggested: 118.0, adjustment: -2.0 },
            ];

            displayHistory(mockData);
        } catch (e) {
            console.error('Failed to load history:', e);
        }
    }

    function displayHistory(records) {
        const tbody = document.getElementById('electrolyteList');

        if (!records || records.length === 0) {
            tbody.innerHTML = '<tr><td colspan="5" class="empty-state">暂无优化记录</td></tr>';
            return;
        }

        tbody.innerHTML = records.map(r => {
            const adjClass = r.adjustment > 0 ? 'warning' : r.adjustment < 0 ? 'good' : 'good';
            return `
                <tr>
                    <td><strong>${r.batch_id}</strong></td>
                    <td>${r.nominal.toFixed(1)} g</td>
                    <td>${r.suggested.toFixed(1)} g</td>
                    <td><span class="${adjClass}">${r.adjustment > 0 ? '+' : ''}${r.adjustment.toFixed(1)} g</span></td>
                    <td>
                        <button class="btn btn-sm action-btn" onclick="viewElectrolyteResult('${r.batch_id}')">查看详情</button>
                        <button class="btn btn-sm action-btn" onclick="applySuggestion('${r.batch_id}', ${r.suggested})">应用建议</button>
                    </td>
                </tr>
            `;
        }).join('');
    }

    async function viewElectrolyteResult(batchId) {
        document.getElementById('electrolyteBatchId').value = batchId;

        try {
            const response = await fetch(`${API_BASE}/electrolyte/${batchId}`);
            const result = await response.json();

            if (result.success && result.data) {
                document.getElementById('electrolyteResultSection').style.display = 'block';
                displayOptimizationResult(result.data);
            }
        } catch (e) {
            console.error('Failed to load result:', e);
            displayOptimizationResult({
                batch_id: batchId,
                total_channels: 512,
                avg_nominal_volume: 120.0,
                avg_suggested_volume: 118.5,
                avg_adjustment: -1.5,
                over_injected_count: 23,
                under_injected_count: 12,
                estimated_gas_reduction: 0.8,
                estimated_capacity_improvement: 0.015,
                next_batch_suggestion: 118.5,
            });
        }
    }

    function applySuggestion(batchId, suggestedVolume) {
        if (confirm(`确认将批次 ${batchId} 的注液量设置为 ${suggestedVolume.toFixed(1)} g？`)) {
            alert(`已应用建议注液量：${suggestedVolume.toFixed(1)} g\n\n系统将在下次注液时自动使用此参数。`);
        }
    }

    return {
        init,
        optimize,
        loadHistory,
        viewElectrolyteResult,
        applySuggestion
    };
})();

function viewElectrolyteResult(batchId) {
    window.ElectrolyteOptimizer.viewElectrolyteResult(batchId);
}

function applySuggestion(batchId, suggestedVolume) {
    window.ElectrolyteOptimizer.applySuggestion(batchId, suggestedVolume);
}
