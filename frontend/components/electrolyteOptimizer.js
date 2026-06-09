const API_BASE = 'http://localhost:8080/api/v2/electrolyte';

window.ElectrolyteOptimizer = (function() {

    let currentTaskId = null;
    let pollInterval = null;

    function init() {
        console.log('Electrolyte Optimizer v2 module initialized');
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
        showAsyncStatus('pending', '正在提交优化请求...');

        try {
            const response = await fetch(`${API_BASE}/optimize`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    batch_id: batchId,
                    target_gas_volume: targetGasVolume,
                    nominal_volume: nominalVolume,
                })
            });

            const result = await response.json();

            if (result.success && result.data && result.data.task_id) {
                currentTaskId = result.data.task_id;
                startTaskPolling(currentTaskId);
            } else {
                throw new Error(result.message || '优化请求提交失败');
            }
        } catch (e) {
            console.error('Optimization request error:', e);
            showAsyncStatus('error', `请求失败: ${e.message}`);
        }
    }

    function startTaskPolling(taskId) {
        showAsyncStatus('processing', '正在分析气体数据并计算优化方案...');

        pollInterval = setInterval(async () => {
            try {
                const response = await fetch(`${API_BASE}/task/${taskId}`);
                const result = await response.json();

                if (result.success) {
                    const status = result.data.status;

                    if (status === 'completed') {
                        clearInterval(pollInterval);
                        pollInterval = null;
                        showAsyncStatus('completed', '优化计算完成');
                        displayResult(result.data.result);
                    } else if (status === 'failed') {
                        clearInterval(pollInterval);
                        pollInterval = null;
                        showAsyncStatus('error', result.data.message || '优化计算失败');
                    } else if (status === 'processing') {
                        const progress = result.data.progress || 0;
                        updateProgress(progress, result.data.message || '正在处理...');
                    }
                }
            } catch (e) {
                console.error('Polling error:', e);
            }
        }, 1000);
    }

    function showAsyncStatus(status, message) {
        const icons = {
            pending: '⏳',
            processing: '⚙️',
            completed: '✅',
            error: '❌'
        };

        const statusClasses = {
            pending: '',
            processing: '',
            completed: 'good',
            error: 'danger'
        };

        document.getElementById('electrolyteSummary').innerHTML = `
            <div class="empty-state">
                ${status === 'processing' ? `
                    <div class="loading-indicator"></div>
                    <div class="progress-bar" style="margin-top:15px; width:200px;">
                        <div class="progress-fill" id="optimizeProgressFill" style="width:0%"></div>
                    </div>
                    <div id="optimizeProgressText" style="margin-top:8px; font-size:12px; color:#888;">0%</div>
                ` : `<div class="empty-state-icon">${icons[status]}</div>`}
                <div style="margin-top:10px; ${statusClasses[status] ? `color:var(--${statusClasses[status]}-color);` : ''}">${message}</div>
            </div>
        `;

        document.getElementById('optimizationDetails').innerHTML = '';
        document.getElementById('confirmSection').style.display = 'none';
    }

    function updateProgress(progress, message) {
        const progressFill = document.getElementById('optimizeProgressFill');
        const progressText = document.getElementById('optimizeProgressText');

        if (progressFill) {
            progressFill.style.width = `${progress}%`;
        }
        if (progressText) {
            progressText.textContent = `${Math.round(progress)}% - ${message}`;
        }
    }

    function displayResult(data) {
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
                    <span class="value">${data.avg_gas_volume.toFixed(2)} mL</span>
                </div>
                <div class="detail-item">
                    <span class="label">产气量标准差</span>
                    <span class="value">${data.gas_volume_std.toFixed(3)} mL</span>
                </div>
                <div class="detail-item">
                    <span class="label">最大产气量</span>
                    <span class="value warning">${data.max_gas_volume.toFixed(2)} mL</span>
                </div>
                <div class="detail-item">
                    <span class="label">最小产气量</span>
                    <span class="value">${data.min_gas_volume.toFixed(2)} mL</span>
                </div>
            </div>
            <div class="optimization-detail-card">
                <h5>优化算法参数</h5>
                <div class="detail-item">
                    <span class="label">气体-电解液转换率</span>
                    <span class="value">${data.conversion_rate || 0.85}</span>
                </div>
                <div class="detail-item">
                    <span class="label">学习率</span>
                    <span class="value">${data.learning_rate || 0.3}</span>
                </div>
                <div class="detail-item">
                    <span class="label">趋势权重</span>
                    <span class="value">${data.trend_weight || 0.2}</span>
                </div>
                <div class="detail-item">
                    <span class="label">最大调整比例</span>
                    <span class="value">±${(data.max_adjust_ratio * 100 || 10)}%</span>
                </div>
            </div>
        `;

        document.getElementById('optimizationDetails').innerHTML = detailsHtml;
        document.getElementById('confirmSection').style.display = 'block';
        document.getElementById('electrolyteResultSection').scrollIntoView({ behavior: 'smooth' });
    }

    async function confirmInjection() {
        const batchId = document.getElementById('electrolyteBatchId').value.trim();
        const suggestedVolume = parseFloat(document.querySelector('#electrolyteSummary .value.good, #electrolyteSummary .value.warning')?.textContent || '0');

        if (!batchId) {
            alert('请先执行优化计算');
            return;
        }

        if (!confirm(`确认将批次 ${batchId} 的注液量设置为 ${suggestedVolume.toFixed(1)} g？\n\n此操作将更新注液机参数并记录到系统中。`)) {
            return;
        }

        try {
            const response = await fetch(`${API_BASE}/confirm`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    batch_id: batchId,
                    confirmed_volume: suggestedVolume,
                    task_id: currentTaskId
                })
            });

            const result = await response.json();

            if (result.success) {
                alert(`✓ 确认成功\n\n批次: ${batchId}\n确认注液量: ${suggestedVolume.toFixed(1)} g\n\n系统已更新注液参数，将在下次注液时生效。`);
                document.getElementById('confirmSection').style.display = 'none';
            } else {
                throw new Error(result.message || '确认失败');
            }
        } catch (e) {
            console.error('Confirm error:', e);
            alert(`确认失败: ${e.message}`);
        }
    }

    return {
        init,
        optimize,
        confirmInjection,
        displayResult
    };
})();

function optimizeElectrolyte() {
    window.ElectrolyteOptimizer.optimize();
}

function confirmElectrolyteInjection() {
    window.ElectrolyteOptimizer.confirmInjection();
}
