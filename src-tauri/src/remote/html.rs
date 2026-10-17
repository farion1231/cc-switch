//! Embedded HTML page for the remote management web UI

pub const REMOTE_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>CC Switch Remote</title>
<link rel="icon" type="image/png" href="/api/icon">
<style>
:root {
    --background: #ffffff;
    --foreground: #0a0a0a;
    --card: #ffffff;
    --card-hover: #f9fafb;
    --muted: #f3f4f6;
    --muted-fg: #6b7280;
    --border: #e5e7eb;
    --primary: #0a84ff;
    --primary-fg: #ffffff;
    --primary-muted: rgba(10, 132, 255, 0.1);
    --success: #10b981;
    --success-muted: rgba(16, 185, 129, 0.1);
    --error: #ef4444;
    --info: #3b82f6;
    --radius: 12px;
    --shadow: 0 1px 2px 0 rgb(0 0 0 / 0.05);
}
@media (prefers-color-scheme: dark) {
    :root {
        --background: #1c1c1e;
        --foreground: #fafafa;
        --card: #262628;
        --card-hover: #2c2c2e;
        --muted: #2e2e30;
        --muted-fg: #9ca3af;
        --border: #3a3a3c;
        --primary: #0a84ff;
        --primary-fg: #ffffff;
        --primary-muted: rgba(10, 132, 255, 0.15);
        --success: #34d399;
        --success-muted: rgba(52, 211, 153, 0.15);
        --error: #f87171;
        --info: #60a5fa;
        --shadow: 0 1px 2px 0 rgb(0 0 0 / 0.3);
    }
}
* { box-sizing: border-box; margin: 0; padding: 0; }
body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
    background: var(--background);
    color: var(--foreground);
    min-height: 100vh;
    padding: 16px;
    transition: background 0.3s, color 0.3s;
}
.header {
    text-align: center;
    padding: 20px 0 16px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 20px;
}
.header h1 {
    font-size: 20px;
    color: var(--foreground);
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
}
.header h1 img {
    width: 28px;
    height: 28px;
    border-radius: 6px;
}
.header .current {
    margin-top: 6px;
    font-size: 13px;
    color: var(--muted-fg);
}
.header .current span {
    color: var(--success);
    font-weight: 600;
}
.provider-list { max-width: 500px; margin: 0 auto; }
.provider-card {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 14px 16px;
    margin-bottom: 10px;
    cursor: pointer;
    transition: all 0.2s;
    display: flex;
    align-items: center;
    justify-content: space-between;
    box-shadow: var(--shadow);
}
.provider-card:hover {
    border-color: var(--primary);
    background: var(--card-hover);
}
.provider-card.active {
    border-color: var(--primary);
    background: var(--primary-muted);
    cursor: default;
}
.provider-card.active .status {
    background: var(--primary);
    color: var(--primary-fg);
}
.provider-card .info { display: flex; align-items: center; gap: 12px; min-width: 0; }
.provider-card .icon {
    width: 36px; height: 36px;
    border-radius: 8px;
    background: var(--muted);
    border: 1px solid var(--border);
    display: flex; align-items: center; justify-content: center;
    font-size: 14px;
    font-weight: 600;
    color: var(--foreground);
    overflow: hidden;
    flex-shrink: 0;
}
.provider-card .icon img {
    width: 22px; height: 22px;
    object-fit: contain;
}
.provider-card .name { font-weight: 500; font-size: 15px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.provider-card .category { font-size: 12px; color: var(--muted-fg); margin-top: 1px; }
.provider-card .status {
    font-size: 12px;
    padding: 3px 10px;
    border-radius: 20px;
    background: var(--muted);
    color: var(--muted-fg);
    font-weight: 500;
    white-space: nowrap;
    flex-shrink: 0;
}
.toast {
    position: fixed;
    bottom: 30px;
    left: 50%;
    transform: translateX(-50%) translateY(100px);
    background: var(--success);
    color: #000;
    padding: 10px 22px;
    border-radius: 8px;
    font-weight: 600;
    font-size: 14px;
    transition: transform 0.3s;
    z-index: 100;
}
.toast.show { transform: translateX(-50%) translateY(0); }
.toast.error { background: var(--error); color: #fff; }
.toast.info { background: var(--info); color: #fff; }
.refresh-btn {
    display: block;
    margin: 20px auto;
    background: var(--card);
    border: 1px solid var(--border);
    color: var(--muted-fg);
    padding: 8px 20px;
    border-radius: 8px;
    cursor: pointer;
    font-size: 13px;
    transition: all 0.2s;
}
.refresh-btn:hover { border-color: var(--primary); color: var(--foreground); }
/*
 * 配对面板。只在 `/api/providers` 回 401 时出现 —— 也就是「页面拿到了、但还没
 * 凭据」这一态。远端浏览器唯一的入口就是这里：GET / 是 Public 外壳，其余路由
 * 都要会话 cookie，而 cookie 只能由 POST /pairing/poll 的 Set-Cookie 落地。
 */
.pairing {
    max-width: 500px;
    margin: 40px auto;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 24px 20px;
    text-align: center;
    box-shadow: var(--shadow);
}
.pairing h2 { font-size: 17px; margin-bottom: 8px; }
.pairing p { font-size: 13px; color: var(--muted-fg); line-height: 1.5; }
.pairing .code {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 30px;
    font-weight: 700;
    letter-spacing: 4px;
    margin: 18px 0;
    color: var(--primary);
    word-break: break-all;
}
.pairing .code.pending { color: var(--muted-fg); letter-spacing: 2px; }
.pairing .spinner { font-size: 13px; color: var(--muted-fg); margin-top: 6px; }
.pairing .retry { margin-top: 16px; }
.pairing.error .code { color: var(--error); }
</style>
</head>
<body>
<div class="header">
    <h1><img src="/api/icon" alt="CC Switch"> CC Switch Remote</h1>
    <div class="current">Current: <span id="currentName">-</span></div>
</div>
<div class="pairing" id="pairingPanel" hidden>
    <h2 id="pairingTitle">Pair this device</h2>
    <p id="pairingHint"></p>
    <div class="code pending" id="pairingCode">------</div>
    <div class="spinner" id="pairingSpinner"></div>
    <button class="refresh-btn retry" id="pairingBtn" onclick="startPairing()" hidden>Pair this device</button>
</div>
<div class="provider-list" id="providerList"></div>
<button class="refresh-btn" id="refreshBtn" onclick="loadProviders()">Refresh</button>
<div class="toast" id="toast"></div>
<script>
function showToast(msg, type) {
    const t = document.getElementById('toast');
    t.textContent = msg;
    t.className = 'toast show' + (type ? ' ' + type : '');
    setTimeout(() => t.className = 'toast', 2500);
}

async function loadProviders() {
    try {
        const res = await fetch('/api/providers');
        // 401 = 页面拿到了，但这个设备还没有凭据。远端浏览器的正常起点就是
        // 这里（GET / 是 Public 外壳，业务路由要会话 cookie）。
        if (res.status === 401) { showPairing(); return false; }
        if (!res.ok) { showToast('Failed to load: HTTP ' + res.status, 'error'); return false; }
        const data = await res.json();
        hidePairing();
        const list = document.getElementById('providerList');
        const currentEl = document.getElementById('currentName');
        let currentName = '-';
        list.innerHTML = '';
        (data.providers || []).forEach(p => {
            if (p.is_current) currentName = p.name;
            const card = document.createElement('div');
            card.className = 'provider-card' + (p.is_current ? ' active' : '');
            const initial = (p.name || '?')[0].toUpperCase();
            const color = (p.icon_color && p.icon_color !== 'currentColor') ? p.icon_color : '#e0e0e0';
            const colorParam = encodeURIComponent(color);
            const iconHtml = p.icon
                ? `<img src="/api/provider-icons/${p.icon}?color=${colorParam}" alt="${p.name}" onerror="this.parentElement.innerHTML='${initial}'">`
                : `<span>${initial}</span>`;
            card.innerHTML = `
                <div class="info">
                    <div class="icon">${iconHtml}</div>
                    <div>
                        <div class="name">${p.name}</div>
                        ${p.category ? '<div class="category">' + p.category + '</div>' : ''}
                    </div>
                </div>
                <div class="status">${p.is_current ? 'Active' : 'Switch'}</div>
            `;
            if (!p.is_current) {
                card.onclick = () => switchTo(p.id, p.name);
            }
            list.appendChild(card);
        });
        currentEl.textContent = currentName;
        return true;
    } catch (e) {
        showToast('Failed to load: ' + e.message, 'error');
        return false;
    }
}

async function switchTo(id, name) {
    try {
        const res = await fetch('/api/switch', {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({provider_id: id})
        });
        const data = await res.json();
        if (data.success) {
            showToast('Switched to ' + name);
        } else {
            showToast('Failed: ' + data.error, 'error');
        }
    } catch (e) {
        showToast('Request failed: ' + e.message, 'error');
    }
}

/*
 * 配对流程。远端浏览器唯一的凭据入口：
 *   401 → 用户点「Pair this device」→ POST /pairing/request 拿 userCode
 *       → 桌面端批准 → POST /pairing/poll 的响应带 Set-Cookie
 *       → 浏览器存下 cookie → 整页重开，之后一切请求自动携带。
 * 页面脚本从头到尾拿不到凭据本身（cookie 是 HttpOnly），它只负责驱动流程。
 */
let pairingTimer = null;

function stopPolling() {
    if (pairingTimer !== null) { clearTimeout(pairingTimer); pairingTimer = null; }
}

function setPairingButton(label, handler) {
    const btn = document.getElementById('pairingBtn');
    btn.textContent = label || 'Request a new code';
    // 动作跟着文案一起换。按钮上还挂着 HTML 里写死的内联 `onclick="startPairing()"`，
    // 只改文案不改动作的话，「Retry」点下去会变成「再要一张码」—— 而重试轮询与
    // 重新发起配对是两件不同的事（前者还握着同一张已拿到手的码）。
    btn.onclick = handler || startPairing;
    btn.hidden = false;
}

/** 401 时的首屏：面板亮起，等用户点按钮。不自动发起 —— 每次刷新都弹一次
 *  桌面端审批框会变成噪音。 */
function showPairing() {
    stopPolling();
    const panel = document.getElementById('pairingPanel');
    panel.hidden = false;
    panel.className = 'pairing';
    document.getElementById('pairingTitle').textContent = 'Pair this device';
    document.getElementById('pairingHint').textContent =
        'This device is not paired yet. Approve it once on the host machine and it stays paired.';
    const code = document.getElementById('pairingCode');
    code.textContent = '------';
    code.className = 'code pending';
    document.getElementById('pairingSpinner').textContent = '';
    document.getElementById('refreshBtn').hidden = true;
    document.getElementById('providerList').innerHTML = '';
    document.getElementById('currentName').textContent = '-';
    setPairingButton('Pair this device');
}

/** 失败 / 过期态。与 `showPairing` 分开是因为按钮文案与色调不同。 */
function showPairingError(message) {
    stopPolling();
    const panel = document.getElementById('pairingPanel');
    panel.hidden = false;
    panel.className = 'pairing error';
    document.getElementById('pairingTitle').textContent = 'Pairing failed';
    document.getElementById('pairingHint').textContent = message;
    const code = document.getElementById('pairingCode');
    code.textContent = '------';
    code.className = 'code pending';
    document.getElementById('pairingSpinner').textContent = '';
    setPairingButton('Request a new code');
}

/** 服务端明确回了 429（`status: "rate_limited"`）：这是**确定性**的拒绝，
 *  不是网络抖了一下。如实报错、停止静默轮询，并给一个「拿同一张码继续问」的
 *  按钮 —— 凭据还在手上，桌面端可能已经批了，没有理由逼用户重新配一次。
 *
 *  保留 `pairingCode` 不清空：那张码仍然有效，用户还要照着它在桌面端点批准。 */
function showPairingRateLimited(pollToken, intervalSeconds, expiresAt) {
    stopPolling();
    const panel = document.getElementById('pairingPanel');
    panel.hidden = false;
    panel.className = 'pairing error';
    document.getElementById('pairingTitle').textContent = 'Pairing paused';
    document.getElementById('pairingHint').textContent =
        'The server is rate limiting pairing requests. Wait a moment, then retry.';
    document.getElementById('pairingSpinner').textContent = '';
    setPairingButton('Retry', function () {
        resumePolling(pollToken, intervalSeconds, expiresAt);
    });
}

/** 拿着同一张码回到等待态继续轮询。与 `startPairing` 分开：那条路会去要一张
 *  **新**码，把已经拿到手的凭据和桌面端那次批准一起丢掉。 */
function resumePolling(pollToken, intervalSeconds, expiresAt) {
    stopPolling();
    const panel = document.getElementById('pairingPanel');
    panel.hidden = false;
    panel.className = 'pairing';
    document.getElementById('pairingTitle').textContent = 'Pair this device';
    document.getElementById('pairingHint').textContent =
        'Approve the code below in CC Switch on the host machine.';
    document.getElementById('pairingSpinner').textContent = 'Waiting for approval...';
    document.getElementById('pairingBtn').hidden = true;
    pollPairing(pollToken, intervalSeconds, expiresAt);
}

function hidePairing() {
    stopPolling();
    document.getElementById('pairingPanel').hidden = true;
    document.getElementById('refreshBtn').hidden = false;
}

async function startPairing() {
    stopPolling();
    const panel = document.getElementById('pairingPanel');
    panel.className = 'pairing';
    document.getElementById('pairingTitle').textContent = 'Pair this device';
    document.getElementById('pairingHint').textContent =
        'Approve the code below in CC Switch on the host machine.';
    const code = document.getElementById('pairingCode');
    code.textContent = '------';
    code.className = 'code pending';
    document.getElementById('pairingBtn').hidden = true;
    document.getElementById('pairingSpinner').textContent = 'Requesting a code...';
    try {
        const res = await fetch('/pairing/request', {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({clientName: 'Web UI'})
        });
        if (!res.ok) {
            showPairingError('Could not start pairing: HTTP ' + res.status +
                (res.status === 429 ? ' (too many attempts, wait a moment)' : ''));
            return;
        }
        const data = await res.json();
        code.textContent = data.userCode;
        code.className = 'code';
        document.getElementById('pairingSpinner').textContent = 'Waiting for approval...';
        pollPairing(data.pollToken, data.pollIntervalSeconds, data.expiresAt);
    } catch (e) {
        showPairingError('Could not start pairing: ' + e.message);
    }
}

function pollPairing(pollToken, intervalSeconds, expiresAt) {
    const deadline = Date.parse(expiresAt);
    const tick = async () => {
        pairingTimer = null;
        if (Number.isFinite(deadline) && Date.now() > deadline) {
            showPairingError('The code expired before it was approved.');
            return;
        }
        try {
            const res = await fetch('/pairing/poll', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    // 轮询凭据只走 header：放进 URL 会进浏览器历史、代理日志与 Referer。
                    'Authorization': 'Pairing ' + pollToken
                },
                body: '{}'
            });
            const data = await res.json().catch(() => ({}));
            if (data.status === 'approved') {
                // 这一次响应带 Set-Cookie，浏览器已经把它存下来了。整页重开让
                // 外壳带着 cookie 重走一遍；之后导航 / <img> / EventSource 都由
                // 浏览器自动携带，页面脚本不再参与任何凭据传递。
                location.reload();
                return;
            }
            if (data.status === 'rejected') {
                showPairingError('The request was rejected on the host machine.');
                return;
            }
            if (data.status === 'expired') {
                showPairingError('The code expired before it was approved.');
                return;
            }
            if (data.status === 'rate_limited') {
                // 服务端明确说了「你被限流了」。以前这里没有分支，于是这一发被
                // 当成「什么都没发生」继续往下排队 —— 界面永远停在
                // 「Waiting for approval...」，用户批了也不知道。
                showPairingRateLimited(pollToken, intervalSeconds, expiresAt);
                return;
            }
            if (!res.ok) {
                // 服务端**明确**回了错误码（429 已在上面拦住，这里是 401/5xx
                // 之类）：那是它把话说清楚了，与下面 `catch` 里「这一发网络抖了
                // 一下」不是一回事。继续按节奏空转等于把确定性的失败咽掉。
                showPairingError('Pairing failed: HTTP ' + res.status + '.');
                return;
            }
        } catch (e) {
            // 瞬时网络错误不改判：继续按服务端给的节奏轮询。
            // 这条语义只覆盖 `fetch` 抛出来的传输层错误（连不上 / 连接中断 /
            // 响应体读不出来），**不覆盖**上面那些「拿到了响应、状态码不是 2xx」
            // 的确定性失败 —— 那些已经各自 return 了，走不到这里。
        }
        const wait = Math.max(1, Number(intervalSeconds) || 2);
        pairingTimer = setTimeout(tick, wait * 1000);
    };
    tick();
}

function connectSSE() {
    const es = new EventSource('/api/events');
    es.onmessage = function(e) {
        try {
            const data = JSON.parse(e.data);
            if (data.type === 'switch') {
                loadProviders();
                if (document.hidden) {
                    showToast('Provider updated: ' + data.name, 'info');
                }
            } else if (data.type === 'shutdown') {
                es.close();
                document.body.innerHTML = '<div style="display:flex;justify-content:center;align-items:center;height:100vh;padding:20px;font-size:18px;color:var(--muted-fg);text-align:center;">Remote server stopped.<br>Please reopen from CC Switch.</div>';
            }
        } catch(err) {}
    };
    es.onerror = function() {
        es.close();
        // 「服务没了」与「凭据被撤销」在这一层长得一模一样，都只是 SSE 断了。
        // 交给 loadProviders 去分辨：401 会把配对面板亮起来，其余才算真连不上。
        loadProviders().then(function(ok) {
            if (ok) { connectSSE(); return; }
            if (!document.getElementById('pairingPanel').hidden) return;
            document.body.innerHTML = '<div style="display:flex;justify-content:center;align-items:center;height:100vh;padding:20px;font-size:18px;color:var(--muted-fg);text-align:center;">Connection lost.<br>Please reopen from CC Switch.</div>';
        });
    };
}

(async function init() {
    if (await loadProviders()) connectSSE();
})();
</script>
</body>
</html>"##;
