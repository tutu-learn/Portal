//! Simple web UI for generating Kiff Logger tokens.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use rust_apps_core::AppState;

fn extract_cookie_value(header: &str, name: &str) -> Option<String> {
    for pair in header.split(';') {
        let pair = pair.trim();
        if let Some((key, value)) = pair.split_once('=') {
            if key.trim() == name {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

async fn session_user_from_cookie(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<String> {
    let cookie_header = headers.get("cookie").and_then(|h| h.to_str().ok())?;
    let sid = extract_cookie_value(cookie_header, "sid")?;
    let pool = state.pools.iter().next().map(|e| e.value().clone())?;
    let store = session::SessionStore::new();
    match store.get(&pool, &sid).await {
        Ok(Some(session)) if !session.is_expired() => Some(session.user),
        _ => None,
    }
}

async fn require_kiff_logs_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<String, StatusCode> {
    let user = session_user_from_cookie(state, headers)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let pool = state
        .pools
        .iter()
        .next()
        .map(|e| e.value().clone())
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let pm = permissions::PermissionEngine::new();
    let roles = pm.get_roles(&pool, &user).await.map_err(|_| StatusCode::FORBIDDEN)?;
    if !roles.iter().any(|r| r == "Kiff Logs Admin") {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(user)
}

const TOKEN_UI_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Kiff Logger Token Generator</title>
  <style>
    :root {
      --bg: #f4f5f7;
      --surface: #ffffff;
      --text: #1f272e;
      --muted: #687178;
      --border: #d1d8dd;
      --primary: #2490ef;
      --primary-dark: #1879ce;
      --danger: #e24c4c;
      --danger-dark: #c53e3e;
      --success: #28a745;
      --warning: #f5833a;
      --radius: 10px;
      --shadow: 0 1px 3px rgba(0,0,0,0.08), 0 1px 2px rgba(0,0,0,0.04);
    }
    * { box-sizing: border-box; }
    body {
      font-family: Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
      background: var(--bg);
      color: var(--text);
      margin: 0;
      padding: 0;
      line-height: 1.5;
    }
    .header {
      background: var(--surface);
      border-bottom: 1px solid var(--border);
      padding: 1.25rem 2rem;
      box-shadow: var(--shadow);
    }
    .header-inner {
      max-width: 1100px;
      margin: 0 auto;
      display: flex;
      align-items: center;
      gap: 0.75rem;
    }
    .header-icon {
      width: 2.25rem;
      height: 2.25rem;
      background: var(--primary);
      color: #fff;
      border-radius: var(--radius);
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 1.1rem;
    }
    .header h1 {
      margin: 0;
      font-size: 1.25rem;
      font-weight: 600;
    }
    .header p {
      margin: 0;
      color: var(--muted);
      font-size: 0.875rem;
    }
    .container {
      max-width: 1100px;
      margin: 0 auto;
      padding: 1.5rem 2rem;
    }
    .grid {
      display: grid;
      grid-template-columns: 380px 1fr;
      gap: 1.5rem;
      align-items: start;
    }
    @media (max-width: 900px) {
      .grid { grid-template-columns: 1fr; }
    }
    .panel {
      background: var(--surface);
      border-radius: var(--radius);
      box-shadow: var(--shadow);
      padding: 1.5rem;
      border: 1px solid var(--border);
    }
    .panel h2 {
      margin: 0 0 0.25rem;
      font-size: 1rem;
      font-weight: 600;
    }
    .panel .subtitle {
      margin: 0 0 1.25rem;
      color: var(--muted);
      font-size: 0.875rem;
    }
    label {
      display: block;
      margin-bottom: 0.35rem;
      font-weight: 500;
      font-size: 0.8125rem;
      color: var(--text);
    }
    input, textarea, select {
      width: 100%;
      padding: 0.55rem 0.75rem;
      border: 1px solid var(--border);
      border-radius: 6px;
      font-size: 0.9375rem;
      margin-bottom: 1rem;
      background: #fff;
      transition: border-color 0.15s, box-shadow 0.15s;
    }
    input:focus, textarea:focus, select:focus {
      outline: none;
      border-color: var(--primary);
      box-shadow: 0 0 0 3px rgba(36,144,239,0.12);
    }
    textarea { resize: vertical; min-height: 4rem; }
    .btn {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 0.35rem;
      border: none;
      border-radius: 6px;
      padding: 0.6rem 1rem;
      font-size: 0.9375rem;
      font-weight: 500;
      cursor: pointer;
      transition: background 0.15s, transform 0.05s;
    }
    .btn:active { transform: translateY(1px); }
    .btn:disabled { opacity: 0.6; cursor: not-allowed; }
    .btn-primary { background: var(--primary); color: #fff; }
    .btn-primary:hover { background: var(--primary-dark); }
    .btn-danger { background: var(--danger); color: #fff; }
    .btn-danger:hover { background: var(--danger-dark); }
    .btn-outline { background: #fff; color: var(--primary); border: 1px solid var(--primary); }
    .btn-outline:hover { background: #f0f8ff; }
    .btn-small { padding: 0.35rem 0.6rem; font-size: 0.8125rem; }
    .alert {
      margin-top: 1rem;
      padding: 0.875rem 1rem;
      border-radius: 6px;
      font-size: 0.9375rem;
      display: none;
    }
    .alert.visible { display: block; }
    .alert-success { background: #eafaf1; border: 1px solid #b8e6c8; color: #155724; }
    .alert-error { background: #fff0f0; border: 1px solid #f5b5b5; color: #721c24; }
    .alert-info { background: #eef7ff; border: 1px solid #b3d7f7; color: #0c5460; }
    .token-box {
      width: 100%;
      font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
      font-size: 0.875rem;
      padding: 0.75rem;
      border: 1px solid var(--border);
      border-radius: 6px;
      background: #f8f9fa;
      word-break: break-all;
      resize: none;
      margin-top: 0.5rem;
    }
    .table-wrap { overflow-x: auto; }
    .token-table {
      width: 100%;
      border-collapse: separate;
      border-spacing: 0;
      font-size: 0.875rem;
      margin-top: 0.5rem;
    }
    .token-table th, .token-table td {
      padding: 0.65rem 0.75rem;
      border-bottom: 1px solid var(--border);
      text-align: left;
      vertical-align: middle;
    }
    .token-table th {
      background: #f8f9fa;
      font-weight: 600;
      color: var(--muted);
      font-size: 0.8125rem;
      text-transform: uppercase;
      letter-spacing: 0.03em;
    }
    .token-table tr:last-child td { border-bottom: none; }
    .token-table tr:hover td { background: #fafbfc; }
    .token-table tr.revoked td { background: #fff5f5; color: #9b2c2c; }
    .token-table tr.revoked:hover td { background: #ffeeee; }
    .badge {
      display: inline-block;
      padding: 0.2rem 0.55rem;
      border-radius: 999px;
      font-size: 0.75rem;
      font-weight: 600;
    }
    .badge-active { background: #eafaf1; color: #155724; }
    .badge-revoked { background: #fff0f0; color: #721c24; }
    code {
      font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
      font-size: 0.85rem;
      background: #f4f5f7;
      padding: 0.15rem 0.35rem;
      border-radius: 4px;
    }
    .empty-state {
      text-align: center;
      padding: 2rem;
      color: var(--muted);
    }
  </style>
</head>
<body>
  <header class="header">
    <div class="header-inner">
      <div class="header-icon">🔑</div>
      <div>
        <h1>Kiff Logger Token UI</h1>
        <p>Create and revoke bearer tokens for external log ingestion</p>
      </div>
    </div>
  </header>

  <main class="container">
    <div class="grid">
      <section class="panel">
        <h2>Generate token</h2>
        <p class="subtitle">The raw token is shown only once.</p>

        <form id="token-form">
          <label for="token_name">Token name</label>
          <input type="text" id="token_name" name="token_name" placeholder="e.g. Production API" required>

          <label for="external_app">External app / system</label>
          <input type="text" id="external_app" name="external_app" placeholder="e.g. payment-gateway">

          <label for="role">Role granted by token</label>
          <select id="role" name="role">
            <option value="Kiff Logs" selected>Kiff Logs</option>
          </select>

          <label for="description">Description</label>
          <textarea id="description" name="description" placeholder="Optional note"></textarea>

          <button type="submit" id="submit-btn" class="btn btn-primary">Generate token</button>
        </form>

        <div id="result" class="alert alert-info">
          <label style="font-weight: 600; display: block; margin-bottom: 0.25rem;">Token — copy it now, it will not be shown again</label>
          <textarea id="token-box" class="token-box" rows="3" readonly></textarea>
          <div style="margin-top: 0.75rem; display: flex; gap: 0.5rem;">
            <button type="button" id="copy-btn" class="btn btn-outline btn-small">Copy to clipboard</button>
          </div>
          <p id="result-message" style="margin-top: 0.75rem; margin-bottom: 0;"></p>
        </div>
      </section>

      <section class="panel">
        <h2>Existing tokens</h2>
        <p class="subtitle">Active tokens can be blacklisted instantly.</p>
        <div id="token-list-wrap"><p class="empty-state">Loading tokens...</p></div>
      </section>
    </div>
  </main>

  <script>
    const form = document.getElementById('token-form');
    const result = document.getElementById('result');
    const tokenBox = document.getElementById('token-box');
    const resultMessage = document.getElementById('result-message');
    const submitBtn = document.getElementById('submit-btn');
    const copyBtn = document.getElementById('copy-btn');

    function showResult(type, msg) {
      result.classList.remove('alert-info', 'alert-success', 'alert-error', 'visible');
      result.classList.add('visible', type === 'error' ? 'alert-error' : type === 'success' ? 'alert-success' : 'alert-info');
      resultMessage.textContent = msg;
    }

    form.addEventListener('submit', async (e) => {
      e.preventDefault();
      result.classList.remove('visible');
      submitBtn.disabled = true;
      submitBtn.textContent = 'Generating...';

      try {
        const res = await fetch('/api/method/kiff_logger.create_token', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            token_name: form.token_name.value,
            external_app: form.external_app.value,
            role: form.role.value,
            description: form.description.value
          })
        });
        const data = await res.json();
        if (!res.ok || data.error || (data.message && !data.message.ok)) {
          throw new Error(data.error || data.exc_type || 'Failed to create token');
        }
        tokenBox.value = data.message.token;
        showResult('success', data.message.message);
        loadTokens();
        form.reset();
      } catch (err) {
        tokenBox.value = '';
        showResult('error', err.message || 'An unexpected error occurred.');
      } finally {
        submitBtn.disabled = false;
        submitBtn.textContent = 'Generate token';
      }
    });

    copyBtn.addEventListener('click', async () => {
      try {
        await navigator.clipboard.writeText(tokenBox.value);
        copyBtn.textContent = 'Copied!';
      } catch {
        tokenBox.select();
        document.execCommand('copy');
        copyBtn.textContent = 'Copied!';
      }
      setTimeout(() => copyBtn.textContent = 'Copy to clipboard', 1500);
    });

    function formatDate(v) {
      if (!v) return '-';
      const d = new Date(v);
      return isNaN(d) ? v : d.toLocaleString();
    }

    async function loadTokens() {
      const wrap = document.getElementById('token-list-wrap');
      wrap.innerHTML = '<p class="empty-state">Loading tokens...</p>';
      try {
        const fields = 'name,token_name,external_app,role,enabled,last_used_at,revoked_at,revoked_by';
        const res = await fetch('/api/resource/Kiff Logger Token?fields=' + encodeURIComponent(fields) + '&limit=100');
        const data = await res.json();
        if (!res.ok || data.error) throw new Error(data.error || data.exc_type || 'Failed to load tokens');
        const tokens = data.data || [];
        if (!tokens.length) {
          wrap.innerHTML = '<p class="empty-state">No tokens found.</p>';
          return;
        }
        let html = '<div class="table-wrap"><table class="token-table"><thead><tr><th>Prefix</th><th>Name</th><th>External app</th><th>Role</th><th>Status</th><th>Last used</th><th style="width:1%"></th></tr></thead><tbody>';
        for (const t of tokens) {
          const revoked = !t.enabled;
          html += `<tr class="${revoked ? 'revoked' : ''}">
            <td><code>${t.name}</code></td>
            <td>${t.token_name || ''}</td>
            <td>${t.external_app || '-'}</td>
            <td>${t.role || ''}</td>
            <td><span class="badge ${revoked ? 'badge-revoked' : 'badge-active'}">${revoked ? 'Revoked' + (t.revoked_by ? ' by ' + t.revoked_by : '') : 'Active'}</span></td>
            <td>${formatDate(t.last_used_at)}</td>
            <td>${revoked ? '' : `<button type="button" class="btn btn-danger btn-small" data-prefix="${t.name}">Revoke</button>`}</td>
          </tr>`;
        }
        html += '</tbody></table></div>';
        wrap.innerHTML = html;

        wrap.querySelectorAll('button[data-prefix]').forEach(btn => {
          btn.addEventListener('click', async () => {
            const name = btn.getAttribute('data-prefix');
            if (!confirm('Blacklist this token? It will no longer be able to push logs.')) return;
            try {
              const res = await fetch('/api/method/kiff_logger.revoke_token', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ name })
              });
              const data = await res.json();
              if (!res.ok || data.error) throw new Error(data.error || data.exc_type || 'Failed to revoke token');
              loadTokens();
            } catch (err) {
              alert(err.message || 'An unexpected error occurred.');
            }
          });
        });
      } catch (err) {
        wrap.innerHTML = `<p class="empty-state">Error loading tokens: ${err.message}</p>`;
      }
    }

    loadTokens();
  </script>
</body>
</html>"#;

pub async fn token_ui_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    match require_kiff_logs_admin(&state, &headers).await {
        Ok(_) => Html(TOKEN_UI_HTML).into_response(),
        Err(status) => (
            status,
            Html(format!("<h1>{}</h1>", status.as_str())),
        )
            .into_response(),
    }
}
