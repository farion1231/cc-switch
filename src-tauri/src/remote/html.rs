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
</style>
</head>
<body>
<div class="header">
    <h1><img src="/api/icon" alt="CC Switch"> CC Switch Remote</h1>
    <div class="current">Current: <span id="currentName">-</span></div>
</div>
<div class="provider-list" id="providerList"></div>
<button class="refresh-btn" onclick="loadProviders()">Refresh</button>
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
        const data = await res.json();
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
    } catch (e) {
        showToast('Failed to load: ' + e.message, 'error');
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
        // Connection lost (server stopped or network issue): do not auto-reconnect
        document.body.innerHTML = '<div style="display:flex;justify-content:center;align-items:center;height:100vh;padding:20px;font-size:18px;color:var(--muted-fg);text-align:center;">Connection lost.<br>Please reopen from CC Switch.</div>';
    };
}

loadProviders();
connectSSE();
</script>
</body>
</html>"##;
