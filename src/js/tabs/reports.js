import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";

let activityChart = null;
let moodChart = null;

export async function initReports() {
    const today = new Date();
    const lastWeek = new Date(today.getTime() - 7 * 24 * 60 * 60 * 1000);

    const startInput = document.getElementById("report-start-date");
    const endInput = document.getElementById("report-end-date");

    if (startInput && !startInput.value) {
        startInput.value = lastWeek.toISOString().split('T')[0];
    }
    if (endInput && !endInput.value) {
        endInput.value = today.toISOString().split('T')[0];
    }

    document.getElementById("btn-refresh-reports")?.addEventListener("click", refreshReports);

    refreshReports();
}

async function refreshReports() {
    const startDate = document.getElementById("report-start-date").value;
    const endDate = document.getElementById("report-end-date").value;

    await Promise.all([
        loadWeeklySummary(),
        loadActivityReport(startDate, endDate),
        loadMoodReport(startDate, endDate)
    ]);
}

async function loadWeeklySummary() {
    const container = document.getElementById("weekly-summary-content");
    try {
        const report = await invoke("report_weekly");
        container.innerHTML = `
            <p><strong>Period:</strong> ${escapeHtml(report.week_start)} to ${escapeHtml(report.week_end)}</p>
            <ul>
                <li>📝 ${escapeHtml(report.journal_entries)} journal entries written</li>
                <li>🔤 ${escapeHtml(report.total_words)} words recorded</li>
                <li>✅ ${escapeHtml(report.tasks_completed)} tasks finished</li>
                <li>⏳ ${escapeHtml(report.tasks_pending)} tasks still pending</li>
                <li>🎭 Dominant Mood: <strong>${escapeHtml(report.dominant_mood || 'Balanced')}</strong></li>
            </ul>
        `;
    } catch (e) {
        container.innerHTML = `<div class="error">Failed to load weekly review: ${e}</div>`;
    }
}

async function loadActivityReport(startDate, endDate) {
    try {
        const data = await invoke("report_activity", { startDate, endDate });
        renderActivityChart(data);
    } catch (e) {
        console.error("Activity report failed", e);
    }
}

async function loadMoodReport(startDate, endDate) {
    try {
        const data = await invoke("report_mood", { startDate, endDate });
        renderMoodChart(data);
    } catch (e) {
        console.error("Mood report failed", e);
    }
}

function renderActivityChart(data) {
    const ctx = document.getElementById('activity-chart').getContext('2d');
    if (activityChart) activityChart.destroy();

    activityChart = new Chart(ctx, {
        type: 'line',
        data: {
            labels: data.map(r => r.date),
            datasets: [
                {
                    label: 'Journals',
                    data: data.map(r => r.journal_count),
                    borderColor: '#6c8ef7',
                    backgroundColor: 'rgba(108, 142, 247, 0.1)',
                    yAxisID: 'y',
                },
                {
                    label: 'Words',
                    data: data.map(r => r.total_words),
                    borderColor: '#4caf50',
                    backgroundColor: 'rgba(76, 175, 80, 0.1)',
                    yAxisID: 'y1',
                }
            ]
        },
        options: {
            responsive: true,
            maintainAspectRatio: false,
            scales: {
                y: { type: 'linear', position: 'left', ticks: { stepSize: 1 } },
                y1: { type: 'linear', position: 'right', grid: { drawOnChartArea: false } }
            }
        }
    });
}

function renderMoodChart(data) {
    const ctx = document.getElementById('mood-chart').getContext('2d');
    if (moodChart) moodChart.destroy();

    // Aggregate mood counts
    const moodCounts = {};
    data.forEach(r => {
        moodCounts[r.mood] = (moodCounts[r.mood] || 0) + r.count;
    });

    moodChart = new Chart(ctx, {
        type: 'doughnut',
        data: {
            labels: Object.keys(moodCounts),
            datasets: [{
                data: Object.values(moodCounts),
                backgroundColor: [
                    '#6c8ef7', '#ba68c8', '#4caf50', '#ff9800', '#f44336', '#9e9e9e'
                ]
            }]
        },
        options: {
            responsive: true,
            maintainAspectRatio: false,
            plugins: {
                legend: { position: 'bottom' }
            }
        }
    });
}
