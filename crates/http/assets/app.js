const API = '/api';
let currentUser = null;

// Escape a value for safe interpolation into HTML text or attribute context.
const esc = s => String(s ?? '').replace(/[&<>"']/g, c =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

async function api(path, opts = {}) {
    const res = await fetch(API + path, {
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json', ...opts.headers },
        ...opts
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || res.statusText);
    return data;
}

function toast(msg, type = 'success') {
    const el = document.createElement('div');
    el.className = `toast ${type}`;
    el.textContent = msg;
    document.body.appendChild(el);
    setTimeout(() => el.remove(), 3000);
}

// Decode a hash segment without crashing on malformed %-escapes.
const decodeSeg = s => { try { return decodeURIComponent(s); } catch { return s; } };

function route() {
    const hash = location.hash.slice(1) || 'login';
    const [page, ...rest] = hash.split('/').map(decodeSeg);
    if (page === 'login') return renderLogin();
    if (page === 'list') return renderList(rest[0]);
    if (page === 'form') return renderForm(rest[0], rest[1]);
    renderLogin();
}

function renderLogin() {
    document.getElementById('nav-user').innerHTML = '';
    document.getElementById('main').innerHTML = `
        <div class="login-wrap">
            <div class="card login-card">
                <h2>Login to Kiff</h2>
                <form id="login-form">
                    <div class="form-group"><label>Email / Username</label><input name="usr" required></div>
                    <div class="form-group"><label>Password</label><input type="password" name="pwd" required></div>
                    <button type="submit" class="btn btn-primary" style="width:100%">Login</button>
                </form>
            </div>
        </div>`;
    document.getElementById('login-form').onsubmit = async (e) => {
        e.preventDefault();
        const fd = new FormData(e.target);
        try {
            await api('/method/login', {
                method: 'POST',
                body: JSON.stringify(Object.fromEntries(fd))
            });
            currentUser = fd.get('usr');
            location.hash = '#list/TestDocType';
        } catch (err) { toast(err.message, 'error'); }
    };
}

function navUserHtml() {
    return `<span>${esc(currentUser || 'Guest')}</span> <a href="#login" class="btn btn-secondary" style="margin-left:12px;padding:4px 10px;font-size:12px">Logout</a>`;
}

async function renderList(doctype) {
    if (!doctype) doctype = 'TestDocType';
    document.getElementById('nav-user').innerHTML = navUserHtml();
    document.getElementById('main').innerHTML = `
        <div class="card">
            <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:16px">
                <h2>${esc(doctype)}</h2>
                <button class="btn btn-primary" id="new-btn">+ New</button>
            </div>
            <div id="list-table" class="table-wrap"><div class="empty">Loading…</div></div>
        </div>`;
    document.getElementById('new-btn').onclick = () => {
        location.hash = '#form/' + encodeURIComponent(doctype) + '/new';
    };
    try {
        const data = await api(`/resource/${encodeURIComponent(doctype)}`);
        const docs = data.data || [];
        if (!docs.length) {
            document.getElementById('list-table').innerHTML = `<div class="empty">No ${esc(doctype)} records found.<br>Create a table and insert some data via the API.</div>`;
            return;
        }
        const keys = Object.keys(docs[0]).filter(k => !['fields','doctype'].includes(k));
        document.getElementById('list-table').innerHTML = `
            <table>
                <thead><tr>${keys.map(k => `<th>${esc(k)}</th>`).join('')}<th style="width:80px"></th></tr></thead>
                <tbody>
                    ${docs.map((d, i) => `<tr class="doc-row" data-i="${i}" style="cursor:pointer">` +
                        keys.map(k => `<td>${esc(d[k])}</td>`).join('') +
                        `<td><button class="btn btn-danger del-btn" data-i="${i}" style="padding:4px 10px;font-size:12px">Del</button></td>` +
                    `</tr>`).join('')}
                </tbody>
            </table>`;
        document.querySelectorAll('#list-table .doc-row').forEach(row => {
            row.addEventListener('click', () => {
                const d = docs[+row.dataset.i];
                location.hash = '#form/' + encodeURIComponent(doctype) + '/' + encodeURIComponent(d.name);
            });
        });
        document.querySelectorAll('#list-table .del-btn').forEach(btn => {
            btn.addEventListener('click', e => {
                e.stopPropagation();
                const d = docs[+btn.dataset.i];
                deleteDoc(doctype, d.name);
            });
        });
    } catch (err) {
        document.getElementById('list-table').innerHTML = `<div class="empty">Error: ${esc(err.message)}</div>`;
    }
}

async function renderForm(doctype, name) {
    document.getElementById('nav-user').innerHTML = navUserHtml();
    const isNew = name === 'new';
    let doc = {};
    if (!isNew) {
        try { doc = (await api(`/resource/${encodeURIComponent(doctype)}/${encodeURIComponent(name)}`)).data || {}; }
        catch (err) { toast(err.message, 'error'); }
    }
    const fields = doc.fields || {};
    const keys = Object.keys(fields).length ? Object.keys(fields) : ['title','status','description'];
    document.getElementById('main').innerHTML = `
        <div class="card">
            <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:16px">
                <h2>${isNew ? 'New ' + esc(doctype) : esc(doc.name)}</h2>
                <div>
                    <button class="btn btn-secondary" id="back-btn">Back</button>
                    <button class="btn btn-primary" id="save-btn" style="margin-left:8px">Save</button>
                </div>
            </div>
            <form id="doc-form">
                <input type="hidden" name="doctype" value="${esc(doctype)}">
                <div class="form-group"><label>Name</label><input name="name" value="${esc(doc.name || '')}" ${isNew ? '' : 'readonly'}></div>
                ${keys.map(k => `
                    <div class="form-group"><label>${esc(k)}</label><input name="${esc(k)}" value="${esc(fields[k] ?? '')}"></div>
                `).join('')}
            </form>
        </div>`;
    document.getElementById('back-btn').onclick = () => history.back();
    document.getElementById('save-btn').onclick = async () => {
        const fd = new FormData(document.getElementById('doc-form'));
        const payload = {};
        fd.forEach((v, k) => { if (k !== 'doctype') payload[k] = v; });
        try {
            if (isNew) {
                await api(`/resource/${encodeURIComponent(doctype)}`, { method: 'POST', body: JSON.stringify(payload) });
                toast('Created');
            } else {
                await api(`/resource/${encodeURIComponent(doctype)}/${encodeURIComponent(doc.name)}`, { method: 'PUT', body: JSON.stringify(payload) });
                toast('Saved');
            }
            location.hash = `#list/${encodeURIComponent(doctype)}`;
        } catch (err) { toast(err.message, 'error'); }
    };
}

async function deleteDoc(doctype, name) {
    if (!confirm(`Delete ${doctype} ${name}?`)) return;
    try {
        await api(`/resource/${encodeURIComponent(doctype)}/${encodeURIComponent(name)}`, { method: 'DELETE' });
        toast('Deleted');
        route();
    } catch (err) { toast(err.message, 'error'); }
}

window.addEventListener('hashchange', route);
document.addEventListener('DOMContentLoaded', route);
