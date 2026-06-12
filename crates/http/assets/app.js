const API = '/api';
let currentUser = null;

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

function route() {
    const hash = location.hash.slice(1) || 'login';
    const [page, ...rest] = hash.split('/');
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
                    <div class="form-group"><label>Email / Username</label><input name="usr" value="Administrator" required></div>
                    <div class="form-group"><label>Password</label><input type="password" name="pwd" value="admin" required></div>
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

async function renderList(doctype) {
    if (!doctype) doctype = 'TestDocType';
    document.getElementById('nav-user').innerHTML = `<span>${currentUser || 'Guest'}</span> <a href="#login" class="btn btn-secondary" style="margin-left:12px;padding:4px 10px;font-size:12px">Logout</a>`;
    document.getElementById('main').innerHTML = `
        <div class="card">
            <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:16px">
                <h2>${doctype}</h2>
                <button class="btn btn-primary" onclick="location.hash='#form/${doctype}/new'">+ New</button>
            </div>
            <div id="list-table" class="table-wrap"><div class="empty">Loading…</div></div>
        </div>`;
    try {
        const data = await api(`/resource/${doctype}`);
        const docs = data.data || [];
        if (!docs.length) {
            document.getElementById('list-table').innerHTML = `<div class="empty">No ${doctype} records found.<br>Create a table and insert some data via the API.</div>`;
            return;
        }
        const keys = Object.keys(docs[0]).filter(k => !['fields','doctype'].includes(k));
        document.getElementById('list-table').innerHTML = `
            <table>
                <thead><tr>${keys.map(k => `<th>${k}</th>`).join('')}<th style="width:80px"></th></tr></thead>
                <tbody>
                    ${docs.map(d => `<tr onclick="location.hash='#form/${doctype}/${d.name}'">` +
                        keys.map(k => `<td>${d[k] ?? ''}</td>`).join('') +
                        `<td><button class="btn btn-danger" style="padding:4px 10px;font-size:12px" onclick="event.stopPropagation();deleteDoc('${doctype}','${d.name}')">Del</button></td>` +
                    `</tr>`).join('')}
                </tbody>
            </table>`;
    } catch (err) {
        document.getElementById('list-table').innerHTML = `<div class="empty">Error: ${err.message}</div>`;
    }
}

async function renderForm(doctype, name) {
    document.getElementById('nav-user').innerHTML = `<span>${currentUser || 'Guest'}</span> <a href="#login" class="btn btn-secondary" style="margin-left:12px;padding:4px 10px;font-size:12px">Logout</a>`;
    const isNew = name === 'new';
    let doc = {};
    if (!isNew) {
        try { doc = (await api(`/resource/${doctype}/${name}`)).data || {}; }
        catch (err) { toast(err.message, 'error'); }
    }
    const fields = doc.fields || {};
    const keys = Object.keys(fields).length ? Object.keys(fields) : ['title','status','description'];
    document.getElementById('main').innerHTML = `
        <div class="card">
            <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:16px">
                <h2>${isNew ? 'New ' + doctype : doc.name}</h2>
                <div>
                    <button class="btn btn-secondary" onclick="history.back()">Back</button>
                    <button class="btn btn-primary" id="save-btn" style="margin-left:8px">Save</button>
                </div>
            </div>
            <form id="doc-form">
                <input type="hidden" name="doctype" value="${doctype}">
                <div class="form-group"><label>Name</label><input name="name" value="${doc.name || ''}" ${isNew ? '' : 'readonly'}></div>
                ${keys.map(k => `
                    <div class="form-group"><label>${k}</label><input name="${k}" value="${fields[k] ?? ''}"></div>
                `).join('')}
            </form>
        </div>`;
    document.getElementById('save-btn').onclick = async () => {
        const fd = new FormData(document.getElementById('doc-form'));
        const payload = {};
        fd.forEach((v, k) => { if (k !== 'doctype') payload[k] = v; });
        try {
            if (isNew) {
                await api(`/resource/${doctype}`, { method: 'POST', body: JSON.stringify(payload) });
                toast('Created');
            } else {
                await api(`/resource/${doctype}/${doc.name}`, { method: 'PUT', body: JSON.stringify(payload) });
                toast('Saved');
            }
            location.hash = `#list/${doctype}`;
        } catch (err) { toast(err.message, 'error'); }
    };
}

async function deleteDoc(doctype, name) {
    if (!confirm(`Delete ${doctype} ${name}?`)) return;
    try {
        await api(`/resource/${doctype}/${name}`, { method: 'DELETE' });
        toast('Deleted');
        route();
    } catch (err) { toast(err.message, 'error'); }
}

window.addEventListener('hashchange', route);
document.addEventListener('DOMContentLoaded', route);
