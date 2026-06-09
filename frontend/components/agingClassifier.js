const API_BASE_V2 = 'http://localhost:8080/api/v2/classifier';

window.AgingClassifier = (function() {
    let currentRequestId = null;
    let currentChannelId = null;
    let currentCabinetId = null;
    let currentAnalysisData = null;

    function init() {
        console.log('Aging Classifier module initialized');
    }

    async function analyze() {
        const cabinetId = parseInt(document.getElementById('classifierCabinetId').value);
        const channelId = parseInt(document.getElementById('classifierChannelId').value);
        const batteryModel = document.getElementById('classifierBatteryModel').value.trim();
        const cycleIndex = parseInt(document.getElementById('classifierCycleIndex').value) || 1;

        if (isNaN(cabinetId) || isNaN(channelId)) {
            alert('请输入有效的柜号和通道号');
            return;
        }

        currentCabinetId = cabinetId;
        currentChannelId = channelId;

        const resultSection = document.getElementById('classifierResultSection');
        const statusSection = document.getElementById('classifierStatusSection');
        resultSection.style.display = 'none';
        statusSection.style.display = 'block';

        updateStatus('pending', '正在提交分析请求...', 0);

        try {
            const submitResponse = await fetch(`${API_BASE_V2}/analyze`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    cabinet_id: cabinetId,
                    channel_id: channelId,
                    cycle_index: cycleIndex,
                    battery_model: batteryModel || undefined,
                })
            });

            const submitResult = await submitResponse.json();

            if (!submitResult.success || !submitResult.data) {
                throw new Error(submitResult.message || '分析请求提交失败');
            }

            currentRequestId = submitResult.data.request_id;
            updateStatus('processing', '分析请求已提交，正在处理中...', 20);

            await pollAnalysisResult(currentRequestId);

        } catch (e) {
            console.error('Analysis error:', e);
            updateStatus('error', `分析失败: ${e.message}`, 0);
        }
    }

    async function pollAnalysisResult(requestId) {
        const maxAttempts = 30;
        let attempts = 0;

        const poll = async () => {
            attempts++;
            try {
                const response = await fetch(`${API_BASE_V2}/status/${requestId}`);
                const result = await response.json();

                if (result.success && result.data) {
                    const status = result.data.status;

                    if (status === 'completed') {
                        updateStatus('completed', '分析完成', 100);
                        if (result.data.analysis) {
                            currentAnalysisData = result.data;
                            setTimeout(() => {
                                displayResult(result.data);
                            }, 500);
                        }
                        return;
                    } else if (status === 'processing') {
                        const progress = Math.min(20 + attempts * 3, 90);
                        updateStatus('processing', result.data.message || '正在分析dQ/dV曲线...', progress);
                    } else if (status === 'failed') {
                        updateStatus('error', result.data.message || '分析失败', 0);
                        return;
                    } else if (status === 'timeout') {
                        updateStatus('error', '分析超时，请稍后重试', 0);
                        return;
                    }
                }

                if (attempts < maxAttempts) {
                    setTimeout(poll, 2000);
                } else {
                    updateStatus('error', '分析超时，请稍后重试', 0);
                }
            } catch (e) {
                console.error('Polling error:', e);
                if (attempts < maxAttempts) {
                    setTimeout(poll, 2000);
                } else {
                    updateStatus('error', `查询状态失败: ${e.message}`, 0);
                }
            }
        };

        poll();
    }

    function updateStatus(status, message, progress) {
        const statusSection = document.getElementById('classifierStatusSection');
        const statusIcon = document.getElementById('classifierStatusIcon');
        const statusText = document.getElementById('classifierStatusText');
        const progressBar = document.getElementById('classifierProgressBar');
        const progressText = document.getElementById('classifierProgressText');

        statusSection.style.display = 'block';

        const icons = {
            pending: '⏳',
            processing: '🔄',
            completed: '✅',
            error: '❌'
        };

        statusIcon.textContent = icons[status] || '⏳';
        statusText.textContent = message;
        progressBar.style.width = `${progress}%`;
        progressText.textContent = `${progress}%`;

        if (status === 'completed') {
            progressBar.classList.add('success');
        } else if (status === 'error') {
            progressBar.classList.add('error');
        } else {
            progressBar.classList.remove('success', 'error');
        }
    }

    function displayResult(data) {
        const resultSection = document.getElementById('classifierResultSection');
        const statusSection = document.getElementById('classifierStatusSection');

        statusSection.style.display = 'none';
        resultSection.style.display = 'block';

        const analysis = data.analysis;
        const dvdqCurve = data.dvdq_curve || [];

        const modeInfo = getModeInfo(analysis.mode);

        const summaryHtml = `
            <div class="summary-item">
                <span class="label">通道</span>
                <span class="value">柜${analysis.cabinet_id} 通道${analysis.channel_id}</span>
            </div>
            <div class="summary-item">
                <span class="label">循环次数</span>
                <span class="value">第${analysis.cycle_index}次循环</span>
            </div>
            <div class="summary-item">
                <span class="label">电池型号</span>
                <span class="value">${analysis.battery_model || '未指定'}</span>
            </div>
            <div class="summary-item">
                <span class="label">衰减模式</span>
                <span class="value ${modeInfo.class}">${modeInfo.icon} ${modeInfo.name}</span>
            </div>
            <div class="summary-item">
                <span class="label">可信度</span>
                <span class="value ${getConfidenceClass(analysis.confidence)}">${(analysis.confidence * 100).toFixed(1)}%</span>
            </div>
            <div class="summary-item">
                <span class="label">容量衰减率</span>
                <span class="value ${analysis.capacity_fade_rate > 0.02 ? 'danger' : 'good'}">${analysis.capacity_fade_rate.toFixed(3)}%/循环</span>
            </div>
        `;

        document.getElementById('classifierSummary').innerHTML = summaryHtml;

        const scoresHtml = `
            <div class="score-bar-item">
                <div class="score-label">正极衰减风险</div>
                <div class="score-bar">
                    <div class="score-fill" style="width: ${analysis.cathode_score * 100}%; background: ${getScoreColor(analysis.cathode_score)};"></div>
                </div>
                <div class="score-value">${(analysis.cathode_score * 100).toFixed(1)}%</div>
            </div>
            <div class="score-bar-item">
                <div class="score-label">负极衰减风险</div>
                <div class="score-bar">
                    <div class="score-fill" style="width: ${analysis.anode_score * 100}%; background: ${getScoreColor(analysis.anode_score)};"></div>
                </div>
                <div class="score-value">${(analysis.anode_score * 100).toFixed(1)}%</div>
            </div>
            <div class="score-bar-item">
                <div class="score-label">电解液消耗风险</div>
                <div class="score-bar">
                    <div class="score-fill" style="width: ${analysis.electrolyte_score * 100}%; background: ${getScoreColor(analysis.electrolyte_score)};"></div>
                </div>
                <div class="score-value">${(analysis.electrolyte_score * 100).toFixed(1)}%</div>
            </div>
            <div class="score-bar-item">
                <div class="score-label">SEI膜生长风险</div>
                <div class="score-bar">
                    <div class="score-fill" style="width: ${analysis.sei_score * 100}%; background: ${getScoreColor(analysis.sei_score)};"></div>
                </div>
                <div class="score-value">${(analysis.sei_score * 100).toFixed(1)}%</div>
            </div>
        `;

        document.getElementById('classifierScores').innerHTML = scoresHtml;

        const peaksHtml = analysis.peak_positions && analysis.peak_positions.length > 0
            ? analysis.peak_positions.map((pos, i) => `
                <div class="peak-item">
                    <span class="peak-position">${pos.toFixed(3)} V</span>
                    <span class="peak-height">强度: ${analysis.peak_heights[i] ? analysis.peak_heights[i].toFixed(4) : 'N/A'}</span>
                </div>
            `).join('')
            : '<div style="color:#888;">未检测到明显特征峰</div>';

        document.getElementById('classifierPeaks').innerHTML = peaksHtml;

        const recommendationsHtml = analysis.recommendations
            ? analysis.recommendations.split('\n').map(line => `<div>${line}</div>`).join('')
            : '<div>暂无建议</div>';

        document.getElementById('classifierRecommendations').innerHTML = recommendationsHtml;

        if (analysis.requires_manual_confirmation) {
            document.getElementById('classifierLabelSection').style.display = 'block';
            document.getElementById('classifierLabelWarning').style.display = 'block';
        } else {
            document.getElementById('classifierLabelSection').style.display = 'block';
            document.getElementById('classifierLabelWarning').style.display = 'none';
        }

        document.getElementById('classifierCorrectedMode').value = analysis.mode || 'normal';

        renderDqDvChart(dvdqCurve, analysis.peak_positions || []);

        if (analysis.used_transfer_learning) {
            document.getElementById('classifierTransferInfo').style.display = 'block';
            document.getElementById('classifierTransferSource').textContent = analysis.transfer_source_model || '未知';
            document.getElementById('classifierTransferSimilarity').textContent = 
                analysis.transfer_similarity ? (analysis.transfer_similarity * 100).toFixed(1) + '%' : 'N/A';
        } else {
            document.getElementById('classifierTransferInfo').style.display = 'none';
        }

        if (analysis.is_new_model) {
            document.getElementById('classifierNewModelWarning').style.display = 'block';
        } else {
            document.getElementById('classifierNewModelWarning').style.display = 'none';
        }

        if (analysis.manually_corrected_mode) {
            const correctedInfo = document.getElementById('classifierCorrectedInfo');
            correctedInfo.style.display = 'block';
            const correctedMode = getModeInfo(analysis.manually_corrected_mode);
            document.getElementById('classifierCorrectedModeDisplay').textContent = correctedMode.name;
            document.getElementById('classifierCorrectedBy').textContent = analysis.corrected_by || '未知';
            document.getElementById('classifierCorrectedAt').textContent = analysis.corrected_at 
                ? new Date(analysis.corrected_at).toLocaleString('zh-CN') 
                : '未知';
            document.getElementById('classifierCorrectedNotes').textContent = analysis.correction_notes || '无';
        } else {
            document.getElementById('classifierCorrectedInfo').style.display = 'none';
        }

        resultSection.scrollIntoView({ behavior: 'smooth' });
    }

    function renderDqDvChart(curveData, peakPositions) {
        const canvas = document.getElementById('dqdqChart');
        if (!canvas || curveData.length === 0) return;

        const ctx = canvas.getContext('2d');
        const width = canvas.width;
        const height = canvas.height;

        ctx.clearRect(0, 0, width, height);

        const padding = { left: 60, right: 20, top: 30, bottom: 50 };
        const chartWidth = width - padding.left - padding.right;
        const chartHeight = height - padding.top - padding.bottom;

        const voltages = curveData.map(p => p.voltage);
        const dqdvValues = curveData.map(p => p.dq_dv);

        const minVoltage = Math.min(...voltages) - 0.1;
        const maxVoltage = Math.max(...voltages) + 0.1;
        const maxDqDv = Math.max(...dqdvValues) * 1.1;
        const minDqDv = 0;

        ctx.strokeStyle = '#333';
        ctx.lineWidth = 1;

        for (let i = 0; i <= 5; i++) {
            const x = padding.left + (chartWidth * i / 5);
            const y = padding.top + chartHeight;
            ctx.beginPath();
            ctx.moveTo(x, padding.top);
            ctx.lineTo(x, y);
            ctx.strokeStyle = '#2a2a2a';
            ctx.stroke();

            const voltage = minVoltage + (maxVoltage - minVoltage) * i / 5;
            ctx.fillStyle = '#888';
            ctx.font = '12px Arial';
            ctx.textAlign = 'center';
            ctx.fillText(voltage.toFixed(1) + 'V', x, y + 20);
        }

        for (let i = 0; i <= 5; i++) {
            const y = padding.top + (chartHeight * i / 5);
            ctx.beginPath();
            ctx.moveTo(padding.left, y);
            ctx.lineTo(padding.left + chartWidth, y);
            ctx.strokeStyle = '#2a2a2a';
            ctx.stroke();

            const dqdv = maxDqDv - (maxDqDv - minDqDv) * i / 5;
            ctx.fillStyle = '#888';
            ctx.font = '12px Arial';
            ctx.textAlign = 'right';
            ctx.fillText(dqdv.toFixed(2), padding.left - 10, y + 4);
        }

        ctx.strokeStyle = '#00d9ff';
        ctx.lineWidth = 2;
        ctx.beginPath();

        curveData.forEach((point, i) => {
            const x = padding.left + chartWidth * (point.voltage - minVoltage) / (maxVoltage - minVoltage);
            const y = padding.top + chartHeight - chartHeight * (point.dq_dv - minDqDv) / (maxDqDv - minDqDv);

            if (i === 0) {
                ctx.moveTo(x, y);
            } else {
                ctx.lineTo(x, y);
            }
        });

        ctx.stroke();

        if (peakPositions && peakPositions.length > 0) {
            peakPositions.forEach(pos => {
                const x = padding.left + chartWidth * (pos - minVoltage) / (maxVoltage - minVoltage);
                if (x >= padding.left && x <= padding.left + chartWidth) {
                    ctx.strokeStyle = '#ff6b6b';
                    ctx.lineWidth = 1;
                    ctx.setLineDash([5, 5]);
                    ctx.beginPath();
                    ctx.moveTo(x, padding.top);
                    ctx.lineTo(x, padding.top + chartHeight);
                    ctx.stroke();
                    ctx.setLineDash([]);

                    ctx.fillStyle = '#ff6b6b';
                    ctx.beginPath();
                    ctx.arc(x, padding.top + 10, 5, 0, Math.PI * 2);
                    ctx.fill();

                    ctx.fillStyle = '#ff6b6b';
                    ctx.font = '11px Arial';
                    ctx.textAlign = 'center';
                    ctx.fillText(pos.toFixed(2) + 'V', x, padding.top - 5);
                }
            });
        }

        ctx.fillStyle = '#aaa';
        ctx.font = '14px Arial';
        ctx.textAlign = 'center';
        ctx.fillText('电压 (V)', width / 2, height - 10);

        ctx.save();
        ctx.translate(15, height / 2);
        ctx.rotate(-Math.PI / 2);
        ctx.fillText('dQ/dV (Ah/V)', 0, 0);
        ctx.restore();

        ctx.fillStyle = '#00d9ff';
        ctx.fillRect(padding.left + 10, padding.top + 10, 20, 3);
        ctx.fillStyle = '#aaa';
        ctx.font = '12px Arial';
        ctx.textAlign = 'left';
        ctx.fillText('dQ/dV 曲线', padding.left + 35, padding.top + 15);

        if (peakPositions && peakPositions.length > 0) {
            ctx.fillStyle = '#ff6b6b';
            ctx.fillRect(padding.left + 150, padding.top + 10, 10, 10);
            ctx.fillStyle = '#aaa';
            ctx.fillText('特征峰位', padding.left + 165, padding.top + 15);
        }
    }

    function getModeInfo(mode) {
        const modes = {
            'normal': { name: '正常', icon: '✅', class: 'good', value: 'normal' },
            'cathode_degradation': { name: '正极衰减', icon: '🔴', class: 'danger', value: 'cathode_degradation' },
            'anode_degradation': { name: '负极衰减', icon: '🔴', class: 'danger', value: 'anode_degradation' },
            'electrolyte_consumption': { name: '电解液消耗', icon: '🔴', class: 'danger', value: 'electrolyte_consumption' },
            'sei_growth': { name: 'SEI膜过度生长', icon: '🟡', class: 'warning', value: 'sei_growth' },
            'mixed': { name: '混合衰减', icon: '🟠', class: 'warning', value: 'mixed' },
            'Normal': { name: '正常', icon: '✅', class: 'good', value: 'normal' },
            'CathodeDegradation': { name: '正极衰减', icon: '🔴', class: 'danger', value: 'cathode_degradation' },
            'AnodeDegradation': { name: '负极衰减', icon: '🔴', class: 'danger', value: 'anode_degradation' },
            'ElectrolyteDepletion': { name: '电解液消耗', icon: '🔴', class: 'danger', value: 'electrolyte_consumption' },
            'ElectrolyteConsumption': { name: '电解液消耗', icon: '🔴', class: 'danger', value: 'electrolyte_consumption' },
            'SEIGrowth': { name: 'SEI膜过度生长', icon: '🟡', class: 'warning', value: 'sei_growth' },
            'Mixed': { name: '混合衰减', icon: '🟠', class: 'warning', value: 'mixed' },
            'MixedDegradation': { name: '混合衰减', icon: '🟠', class: 'warning', value: 'mixed' },
        };
        return modes[mode] || { name: '未知', icon: '❓', class: '', value: 'unknown' };
    }

    function getConfidenceClass(confidence) {
        if (confidence >= 0.8) return 'excellent';
        if (confidence >= 0.6) return 'good';
        if (confidence >= 0.4) return 'warning';
        return 'danger';
    }

    function getScoreColor(score) {
        if (score >= 0.7) return '#ff4444';
        if (score >= 0.5) return '#ffaa00';
        if (score >= 0.3) return '#aaaa00';
        return '#00aa44';
    }

    async function addLabel() {
        if (!currentAnalysisData || !currentAnalysisData.analysis) {
            alert('请先完成分析后再进行标注');
            return;
        }

        const correctedMode = document.getElementById('classifierCorrectedMode').value;
        const notes = document.getElementById('classifierLabelNotes').value.trim();
        const operator = document.getElementById('classifierLabelOperator').value.trim();

        if (!operator) {
            alert('请输入标注人员姓名');
            return;
        }

        const analysis = currentAnalysisData.analysis;

        try {
            const response = await fetch(`${API_BASE_V2}/label`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    cabinet_id: analysis.cabinet_id,
                    channel_id: analysis.channel_id,
                    cycle_index: analysis.cycle_index,
                    corrected_mode: correctedMode,
                    notes,
                    operator,
                })
            });

            const result = await response.json();

            if (result.success) {
                alert('标注提交成功！感谢您的反馈，这将帮助改进模型准确性。');
                document.getElementById('classifierLabelNotes').value = '';
                document.getElementById('classifierLabelSection').style.display = 'none';
            } else {
                throw new Error(result.message || '标注提交失败');
            }
        } catch (e) {
            console.error('Label error:', e);
            alert(`标注提交失败: ${e.message}`);
        }
    }

    async function registerBaseline() {
        const modelName = document.getElementById('baselineModelName').value.trim();
        const cabinetId = parseInt(document.getElementById('baselineCabinetId').value);
        const channelId = parseInt(document.getElementById('baselineChannelId').value);
        const cycleIndex = parseInt(document.getElementById('baselineCycleIndex').value) || 1;

        if (!modelName) {
            alert('请输入电池型号名称');
            return;
        }
        if (isNaN(cabinetId) || isNaN(channelId)) {
            alert('请输入有效的柜号和通道号');
            return;
        }

        try {
            const response = await fetch(`${API_BASE_V2}/baseline`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    model_name: modelName,
                    cabinet_id: cabinetId,
                    channel_id: channelId,
                    cycle_index: cycleIndex,
                })
            });

            const result = await response.json();

            if (result.success) {
                alert(`基线注册成功！\n型号: ${modelName}\n样本将用于后续模型训练和迁移学习。`);
                document.getElementById('baselineModelName').value = '';
                document.getElementById('baselineCabinetId').value = '';
                document.getElementById('baselineChannelId').value = '';
                loadKnownModels();
            } else {
                throw new Error(result.message || '基线注册失败');
            }
        } catch (e) {
            console.error('Register baseline error:', e);
            alert(`基线注册失败: ${e.message}`);
        }
    }

    async function loadKnownModels() {
        try {
            const response = await fetch(`${API_BASE_V2}/models`);
            const result = await response.json();

            if (result.success && result.data) {
                const select = document.getElementById('classifierBatteryModel');
                const currentValue = select.value;
                select.innerHTML = '<option value="">选择电池型号</option>';
                result.data.forEach(model => {
                    const option = document.createElement('option');
                    option.value = model;
                    option.textContent = model;
                    select.appendChild(option);
                });
                select.value = currentValue;
            }
        } catch (e) {
            console.error('Load models error:', e);
        }
    }

    async function loadPendingConfirmations() {
        try {
            const response = await fetch(`${API_BASE_V2}/pending`);
            const result = await response.json();

            if (result.success && result.data) {
                const list = document.getElementById('pendingConfirmationsList');
                if (result.data.length === 0) {
                    list.innerHTML = '<div style="color:#888; text-align:center; padding:20px;">暂无待确认项</div>';
                    return;
                }

                list.innerHTML = result.data.map(item => `
                    <div class="pending-item">
                        <div class="pending-info">
                            <span class="pending-channel">柜${item.cabinet_id} 通道${item.channel_id}</span>
                            <span class="pending-cycle">第${item.cycle_index}次循环</span>
                        </div>
                        <button class="btn btn-sm action-btn" onclick="AgingClassifier.quickLoad(${item.cabinet_id}, ${item.channel_id}, ${item.cycle_index})">
                            查看并标注
                        </button>
                    </div>
                `).join('');
            }
        } catch (e) {
            console.error('Load pending error:', e);
        }
    }

    function quickLoad(cabinetId, channelId, cycleIndex) {
        document.getElementById('classifierCabinetId').value = cabinetId;
        document.getElementById('classifierChannelId').value = channelId;
        document.getElementById('classifierCycleIndex').value = cycleIndex;
        analyze();
    }

    return {
        init,
        analyze,
        addLabel,
        registerBaseline,
        displayResult,
        loadKnownModels,
        loadPendingConfirmations,
        quickLoad
    };
})();

function agingClassifierAnalyze() {
    window.AgingClassifier.analyze();
}

function agingClassifierAddLabel() {
    window.AgingClassifier.addLabel();
}

function agingClassifierRegisterBaseline() {
    window.AgingClassifier.registerBaseline();
}

function agingClassifierLoadPending() {
    window.AgingClassifier.loadPendingConfirmations();
}

function agingClassifierQuickLoad(cabinetId, channelId, cycleIndex) {
    window.AgingClassifier.quickLoad(cabinetId, channelId, cycleIndex);
}
