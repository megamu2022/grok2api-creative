const state = {
  config: null,
  models: [],
  items: [],
  activeId: null,
  filter: "",
  streaming: false,
};

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
const el = (tag, cls, text) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
};

function setupMarkdown() {
  if (!window.marked) return;
  marked.setOptions({
    gfm: true,
    breaks: true,
    highlight(code, lang) {
      if (window.hljs) {
        try {
          if (lang && hljs.getLanguage(lang)) {
            return hljs.highlight(code, { language: lang }).value;
          }
          return hljs.highlightAuto(code).value;
        } catch {
          return escapeHtml(code);
        }
      }
      return escapeHtml(code);
    },
  });
}

function renderMarkdown(text) {
  const raw = text || "";
  if (!window.marked || !window.DOMPurify) {
    return escapeHtml(raw).replace(/\n/g, "<br>");
  }
  const html = marked.parse(raw);
  return DOMPurify.sanitize(html, {
    ADD_ATTR: ["target", "rel", "class"],
  });
}

function applyKatex(container) {
  if (!window.renderMathInElement || !container) return;
  try {
    renderMathInElement(container, {
      delimiters: [
        { left: "$$", right: "$$", display: true },
        { left: "\\[", right: "\\]", display: true },
        { left: "$", right: "$", display: false },
        { left: "\\(", right: "\\)", display: false },
      ],
      throwOnError: false,
    });
  } catch {
    /* ignore */
  }
}

function setMarkdownBody(node, text, { plain = false } = {}) {
  if (!node) return;
  if (plain) {
    node.classList.add("plain");
    node.classList.remove("md");
    node.textContent = text || "";
    return;
  }
  node.classList.remove("plain");
  node.classList.add("md");
  node.innerHTML = renderMarkdown(text || "");
  if (window.hljs) {
    node.querySelectorAll("pre code").forEach((block) => {
      try { hljs.highlightElement(block); } catch { /* ignore */ }
    });
  }
  applyKatex(node);
}

async function api(path, opts = {}) {
  const res = await fetch(path, {
    headers: { "Content-Type": "application/json", ...(opts.headers || {}) },
    ...opts,
  });
  const text = await res.text();
  let data = null;
  try { data = text ? JSON.parse(text) : null; } catch { data = { raw: text }; }
  if (!res.ok) {
    const msg = data?.error?.message || data?.message || text || res.statusText;
    throw new Error(msg);
  }
  return data;
}

function mediaUrl(asset) {
  if (!asset) return "";
  if (asset.local_path) {
    const name = asset.local_path.split(/[/\\]/).pop();
    return `/local/media/${encodeURIComponent(name)}`;
  }
  if (asset.local_url) return asset.local_url;
  return asset.url || "";
}

function kindLabel(kind) {
  return ({ chat: "chat", image: "image", image_edit: "edit", video: "video" })[kind] || kind;
}

function payloadOf(item) {
  if (!item) return null;
  const p = item.payload;
  if (!p) return null;
  if (p.payload_type && p.data) return { type: p.payload_type, data: p.data };
  if (p.messages) return { type: "chat", data: p };
  if (item.kind === "image") return { type: "image", data: p };
  if (item.kind === "image_edit") return { type: "image_edit", data: p };
  if (item.kind === "video") return { type: "video", data: p };
  return { type: item.kind, data: p };
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[c]));
}
function escapeAttr(s) { return escapeHtml(s); }

function normalizeRole(role) {
  if (!role) return "assistant";
  const r = String(role).toLowerCase();
  if (r === "user" || r === "assistant" || r === "system") return r;
  return r;
}

function toolName(t) {
  return t?.name || t?.type_name || t?.type || "tool";
}

function toolStatus(t) {
  return t?.status || "completed";
}

function scrollChatToBottom({ force = false } = {}) {
  const content = $("#content");
  if (!content) return;
  if (!force) {
    const dist = content.scrollHeight - content.scrollTop - content.clientHeight;
    if (dist > 120) return;
  }
  content.scrollTop = content.scrollHeight;
}

function toolCountSummary(tools) {
  const counts = new Map();
  for (const t of tools || []) {
    const name = toolName(t);
    counts.set(name, (counts.get(name) || 0) + 1);
  }
  return [...counts.entries()]
    .map(([name, n]) => (n > 1 ? `${name} ×${n}` : name))
    .join(" · ");
}

function updateLocalMessage(historyId, messageId, patch) {
  const item = state.items.find((x) => x.id === historyId);
  if (!item) return;
  const p = payloadOf(item);
  if (p?.type !== "chat") return;
  const msg = (p.data.messages || []).find((m) => m.id === messageId);
  if (!msg) return;
  Object.assign(msg, patch);
  item.payload = { payload_type: "chat", data: p.data };
}

async function refreshConfig() {
  state.config = await api("/api/config");
  const banner = $("#setup-banner");
  if (!state.config.ready) banner.classList.remove("hidden");
  else banner.classList.add("hidden");
  $("#cfg-base").value = state.config.base_url || "";
  $("#cfg-key").value = "";
  $("#cfg-masked").textContent = state.config.api_key_set
    ? `当前 Key: ${state.config.api_key_masked}`
    : "尚未设置 Key";
  $("#cfg-chat-model").value = state.config.default_chat_model || "";
  $("#cfg-image-model").value = state.config.default_image_model || "";
  $("#cfg-edit-model").value = state.config.default_image_edit_model || "";
  $("#cfg-video-model").value = state.config.default_video_model || "";
}

async function refreshModels() {
  try {
    const data = await api("/api/models");
    state.models = data.data || [];
  } catch (e) {
    state.models = [];
    console.warn(e);
  }
}

async function refreshHistory({ rerender = true } = {}) {
  const data = await api("/api/history");
  state.items = data.items || [];
  renderHistory();
  if (!rerender) return;
  if (state.activeId) {
    const still = state.items.find((x) => x.id === state.activeId);
    if (still) renderMain(still);
    else {
      state.activeId = null;
      renderMain(null);
    }
  }
}

function updateLocalItemMessages(historyId, messages) {
  const item = state.items.find((x) => x.id === historyId);
  if (!item) return;
  const p = payloadOf(item);
  if (p?.type !== "chat") return;
  p.data.messages = messages;
  item.payload = { payload_type: "chat", data: p.data };
}

function renderHistory() {
  const list = $("#history-list");
  list.innerHTML = "";
  const q = state.filter.trim().toLowerCase();
  const items = state.items.filter(
    (it) => !q || (it.title || "").toLowerCase().includes(q) || it.kind.includes(q),
  );
  for (const it of items) {
    const row = el("div", `history-item${it.id === state.activeId ? " active" : ""}`);
    row.innerHTML = `
      <div class="kind">${kindLabel(it.kind)}</div>
      <div class="meta">
        <div class="title"></div>
        <div class="time"></div>
      </div>
      <button class="del" title="删除">✕</button>`;
    row.querySelector(".title").textContent = it.title || "Untitled";
    row.querySelector(".time").textContent = new Date(it.updated_at).toLocaleString();
    row.onclick = (e) => {
      if (e.target.classList.contains("del")) return;
      state.activeId = it.id;
      renderHistory();
      renderMain(it);
    };
    row.querySelector(".del").onclick = async (e) => {
      e.stopPropagation();
      if (!confirm("删除该历史？")) return;
      await api(`/api/history/${it.id}`, { method: "DELETE" });
      if (state.activeId === it.id) state.activeId = null;
      await refreshHistory();
      if (!state.activeId) renderMain(null);
    };
    list.appendChild(row);
  }
}

function modelOptions(filterFn) {
  const models = state.models.filter(filterFn);
  if (!models.length) return `<option value="">（手填或先拉取模型）</option>`;
  return models
    .map((m) => `<option value="${escapeAttr(m.id)}">${escapeHtml(m.id)}</option>`)
    .join("");
}

function renderMain(item) {
  const title = $("#item-title");
  const toolbar = $("#toolbar");
  const content = $("#content");
  const composer = $("#composer");
  toolbar.innerHTML = "";
  content.innerHTML = "";
  composer.innerHTML = "";
  composer.classList.add("hidden");
  content.classList.remove("empty-state");

  if (!item) {
    title.textContent = "选择或新建会话";
    content.classList.add("empty-state");
    content.textContent = "从左侧新建 Chat / Image / Edit / Video";
    return;
  }

  title.textContent = item.title || kindLabel(item.kind);
  const p = payloadOf(item);

  if (p?.type === "chat") renderChat(item, p.data);
  else if (p?.type === "image") renderImage(item, p.data);
  else if (p?.type === "image_edit") renderImageEdit(item, p.data);
  else if (p?.type === "video") renderVideo(item, p.data);
  else content.textContent = "未知类型";
}

function renderChat(item, chat) {
  const toolbar = $("#toolbar");
  toolbar.innerHTML = `
    <label>模型
      <select id="chat-model">${modelOptions((m) => /chat|response|grok/i.test(m.capability + m.id))}</select>
    </label>
    <label><input type="checkbox" id="chat-web" ${chat.web_search ? "checked" : ""}/> Web</label>
    <label><input type="checkbox" id="chat-x" ${chat.x_search ? "checked" : ""}/> X</label>
    <label>思考
      <select id="chat-reason">
        ${["auto", "none", "low", "medium", "high", "xhigh"]
          .map(
            (v) =>
              `<option value="${v}" ${chat.reasoning_effort === v ? "selected" : ""}>${v}</option>`,
          )
          .join("")}
      </select>
    </label>
    <button id="chat-save-settings">应用设置</button>`;

  const modelSel = $("#chat-model");
  if (chat.model) {
    if (![...modelSel.options].some((o) => o.value === chat.model)) {
      modelSel.add(new Option(chat.model, chat.model, true, true));
    }
    modelSel.value = chat.model;
  }

  $("#chat-save-settings").onclick = async () => {
    chat.model = modelSel.value;
    chat.web_search = $("#chat-web").checked;
    chat.x_search = $("#chat-x").checked;
    chat.reasoning_effort = $("#chat-reason").value;
    item.payload = { payload_type: "chat", data: chat };
    await api(`/api/history/${item.id}`, { method: "PUT", body: JSON.stringify(item) });
    await refreshHistory();
  };

  const box = el("div", "messages");
  for (const msg of chat.messages || []) {
    box.appendChild(renderMessage(item, msg));
  }
  $("#content").appendChild(box);
  scrollChatToBottom({ force: true });

  const composer = $("#composer");
  composer.classList.remove("hidden");
  composer.innerHTML = `
    <textarea id="chat-input" placeholder="输入消息… Enter 发送，Shift+Enter 换行" ${state.streaming ? "disabled" : ""}></textarea>
    <div class="row">
      <button id="chat-send" ${state.streaming ? "disabled" : ""}>发送</button>
      <span class="status" id="chat-status"></span>
    </div>`;
  const input = $("#chat-input");
  input.onkeydown = (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendChat(item);
    }
  };
  $("#chat-send").onclick = () => sendChat(item);
}

function renderThinkingBlock(reasoning, { live = false } = {}) {
  const text = reasoning || "";
  if (!text && !live) return null;
  const details = document.createElement("details");
  details.className = "meta-block thinking-block";
  details.open = false;
  const summary = document.createElement("summary");
  const label = el("span", "meta-label", "思考");
  const count = el("span", "meta-count", text ? `${text.length} 字` : live ? "进行中…" : "");
  summary.appendChild(label);
  summary.appendChild(count);
  details.appendChild(summary);
  const body = el("div", "meta-body", text);
  details.appendChild(body);
  return details;
}

function renderToolCards(tools, { live = false } = {}) {
  const list = tools || [];
  if (!list.length && !live) return null;
  const details = document.createElement("details");
  details.className = "meta-block tools-block";
  details.open = false;

  const summary = document.createElement("summary");
  const label = el("span", "meta-label", "工具");
  const countText = list.length
    ? `${list.length} 次 · ${toolCountSummary(list)}`
    : live
      ? "调用中…"
      : "";
  const count = el("span", "meta-count", countText);
  summary.appendChild(label);
  summary.appendChild(count);
  details.appendChild(summary);

  const wrap = el("div", "tools-list");
  for (const t of list) {
    const card = el("div", "tool-card");
    const head = el("div", "tool-head");
    head.appendChild(el("span", `badge ${toolStatus(t)}`, toolStatus(t)));
    head.appendChild(el("span", "tool-name", toolName(t)));
    card.appendChild(head);
    if (t.detail) card.appendChild(el("div", "tool-detail", t.detail));
    wrap.appendChild(card);
  }
  details.appendChild(wrap);
  return details;
}

function renderMessage(item, msg, { live = false } = {}) {
  const role = normalizeRole(msg.role);
  const node = el("div", `msg ${role}${live ? " streaming" : ""}`);
  if (msg.id) node.dataset.id = msg.id;
  if (live) node.dataset.live = "1";
  node.dataset.content = msg.content || "";

  const roleRow = el("div", "role-row");
  roleRow.appendChild(el("div", "role", role === "user" ? "You" : "Assistant"));
  if (live) roleRow.appendChild(el("div", "stream-dot"));
  node.appendChild(roleRow);

  const thinking = renderThinkingBlock(msg.reasoning || "", { live });
  if (thinking) node.appendChild(thinking);

  const tools = renderToolCards(msg.tools || [], { live });
  if (tools) node.appendChild(tools);

  const body = el("div", "body");
  if (role === "assistant") setMarkdownBody(body, msg.content || "");
  else setMarkdownBody(body, msg.content || "", { plain: true });
  node.appendChild(body);

  const editBox = el("div", "edit-box");
  const ta = document.createElement("textarea");
  ta.value = msg.content || "";
  ta.rows = Math.min(16, Math.max(4, (msg.content || "").split("\n").length + 1));
  editBox.appendChild(ta);
  const editActions = el("div", "edit-actions");
  const saveBtn = el("button", null, "保存");
  const saveRetryBtn = el("button", null, "保存并重试");
  const cancelBtn = el("button", null, "取消");
  const hint = el("span", "edit-hint", "Esc 取消 · ⌘/Ctrl+Enter 保存" + (role === "user" ? "并重试" : ""));
  editActions.appendChild(saveBtn);
  if (role === "user") editActions.appendChild(saveRetryBtn);
  editActions.appendChild(cancelBtn);
  editActions.appendChild(hint);
  editBox.appendChild(editActions);
  node.appendChild(editBox);

  const exitEdit = () => {
    node.classList.remove("editing");
    ta.value = node.dataset.content || msg.content || "";
  };

  const applyLocalContent = (next) => {
    node.dataset.content = next;
    msg.content = next;
    if (role === "assistant") setMarkdownBody(body, next);
    else setMarkdownBody(body, next, { plain: true });
    updateLocalMessage(item.id, msg.id, { content: next });
  };

  const doSave = async (resend) => {
    if (state.streaming) return;
    const next = ta.value;
    saveBtn.disabled = true;
    saveRetryBtn.disabled = true;
    try {
      await api(`/api/chat/${item.id}/edit-message`, {
        method: "POST",
        body: JSON.stringify({ message_id: msg.id, content: next, resend }),
      });
      applyLocalContent(next);
      node.classList.remove("editing");
      if (resend) {
        await streamRetry(item.id, msg.id);
      } else {
        await refreshHistory({ rerender: false });
      }
    } catch (e) {
      alert(e.message || String(e));
    } finally {
      saveBtn.disabled = false;
      saveRetryBtn.disabled = false;
    }
  };

  cancelBtn.onclick = exitEdit;
  saveBtn.onclick = () => doSave(false);
  saveRetryBtn.onclick = () => doSave(true);
  ta.onkeydown = (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      exitEdit();
      return;
    }
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      doSave(role === "user");
    }
  };

  if (!live && (role === "user" || role === "assistant") && msg.id) {
    const actions = el("div", "msg-actions");
    const editBtn = el("button", null, "编辑");
    editBtn.onclick = () => {
      if (state.streaming) return;
      node.classList.add("editing");
      ta.value = node.dataset.content || msg.content || "";
      ta.focus();
      ta.setSelectionRange(ta.value.length, ta.value.length);
    };
    actions.appendChild(editBtn);

    const copyBtn = el("button", null, "复制");
    copyBtn.onclick = async () => {
      try {
        await navigator.clipboard.writeText(node.dataset.content || msg.content || "");
        copyBtn.textContent = "已复制";
        setTimeout(() => { copyBtn.textContent = "复制"; }, 1000);
      } catch {
        /* ignore */
      }
    };
    actions.appendChild(copyBtn);

    const delBtn = el("button", null, "删除");
    delBtn.onclick = async () => {
      if (state.streaming) return;
      if (!confirm("删除该消息？")) return;
      await api(`/api/chat/${item.id}/delete-message`, {
        method: "POST",
        body: JSON.stringify({ message_id: msg.id }),
      });
      await refreshHistory();
    };
    actions.appendChild(delBtn);

    if (role === "user") {
      const retryBtn = el("button", null, "重试");
      retryBtn.onclick = () => {
        if (state.streaming) return;
        streamRetry(item.id, msg.id);
      };
      actions.appendChild(retryBtn);
    }
    node.appendChild(actions);
  }

  return node;
}

function updateLiveAssistant(node, snap) {
  if (!node) return;
  const roleRow = node.querySelector(".role-row");

  let thinking = node.querySelector(".thinking-block");
  if (snap.reasoning) {
    if (!thinking) {
      thinking = renderThinkingBlock(snap.reasoning, { live: true });
      if (thinking) {
        const tools = node.querySelector(".tools-block");
        const body = node.querySelector(".body");
        if (tools) tools.before(thinking);
        else if (body) body.before(thinking);
        else roleRow.after(thinking);
      }
    } else {
      thinking.querySelector(".meta-body").textContent = snap.reasoning;
      const count = thinking.querySelector(".meta-count");
      if (count) count.textContent = `${snap.reasoning.length} 字`;
    }
  }

  if (snap.tools?.length) {
    const next = renderToolCards(snap.tools, { live: true });
    const prev = node.querySelector(".tools-block");
    if (prev && next) prev.replaceWith(next);
    else if (next) {
      const body = node.querySelector(".body");
      if (body) body.before(next);
      else node.appendChild(next);
    }
  }

  const body = node.querySelector(".body");
  setMarkdownBody(body, snap.text || "");
  node.dataset.content = snap.text || "";
  scrollChatToBottom();
}

async function persistChatSettings(item) {
  const chat = payloadOf(item).data;
  chat.model = $("#chat-model")?.value || chat.model;
  chat.web_search = $("#chat-web")?.checked ?? chat.web_search;
  chat.x_search = $("#chat-x")?.checked ?? chat.x_search;
  chat.reasoning_effort = $("#chat-reason")?.value || chat.reasoning_effort;
  item.payload = { payload_type: "chat", data: chat };
  await api(`/api/history/${item.id}`, { method: "PUT", body: JSON.stringify(item) });
  return chat;
}

async function sendChat(item) {
  if (state.streaming) return;
  const text = $("#chat-input")?.value?.trim();
  if (!text) return;

  await persistChatSettings(item);

  const box = $("#content .messages");
  // optimistic user bubble
  const userNode = renderMessage(item, {
    id: `tmp-user-${Date.now()}`,
    role: "user",
    content: text,
  });
  // strip actions on temp optimistic node until final history arrives
  userNode.querySelector(".msg-actions")?.remove();
  userNode.querySelector(".edit-box")?.remove();
  box?.appendChild(userNode);

  const assistantNode = renderMessage(
    item,
    { id: `tmp-assistant-${Date.now()}`, role: "assistant", content: "", reasoning: "", tools: [] },
    { live: true },
  );
  assistantNode.querySelector(".msg-actions")?.remove();
  assistantNode.querySelector(".edit-box")?.remove();
  box?.appendChild(assistantNode);
  scrollChatToBottom({ force: true });

  state.streaming = true;
  $("#chat-status").textContent = "生成中…";
  $("#chat-status").classList.remove("error");
  $("#chat-input").value = "";
  $("#chat-input").disabled = true;
  $("#chat-send").disabled = true;

  try {
    await consumeSse(
      "/api/chat/stream",
      { history_id: item.id, content: text },
      item.id,
      { liveNode: assistantNode },
    );
    $("#chat-status").textContent = "";
  } catch (e) {
    $("#chat-status").textContent = e.message;
    $("#chat-status").classList.add("error");
    const body = assistantNode.querySelector(".body");
    if (body && !body.textContent.trim()) {
      setMarkdownBody(body, `Error: ${e.message}`, { plain: true });
    }
  } finally {
    state.streaming = false;
    $("#chat-input").disabled = false;
    $("#chat-send").disabled = false;
    // final authoritative render after history is saved on server
    await refreshHistory();
  }
}

async function streamRetry(historyId, messageId) {
  if (state.streaming) return;
  state.streaming = true;
  const item = state.items.find((x) => x.id === historyId);
  const box = $("#content .messages");
  // remove trailing assistant after this user if present in DOM
  if (box) {
    const userEl = box.querySelector(`.msg.user[data-id="${CSS.escape(messageId)}"]`);
    if (userEl) {
      let n = userEl.nextElementSibling;
      while (n && n.classList.contains("assistant")) {
        const next = n.nextElementSibling;
        n.remove();
        n = next;
      }
    }
  }
  const assistantNode = renderMessage(
    item || { id: historyId },
    { role: "assistant", content: "", tools: [] },
    { live: true },
  );
  assistantNode.querySelector(".msg-actions")?.remove();
  assistantNode.querySelector(".edit-box")?.remove();
  box?.appendChild(assistantNode);
  scrollChatToBottom({ force: true });

  try {
    await consumeSse(
      `/api/chat/${historyId}/retry`,
      { message_id: messageId },
      historyId,
      { liveNode: assistantNode },
    );
  } catch (e) {
    alert(e.message);
  } finally {
    state.streaming = false;
    await refreshHistory();
  }
}

async function consumeSse(url, body, historyId, { liveNode } = {}) {
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "text/event-stream" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const t = await res.text();
    throw new Error(t || res.statusText);
  }
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  let live = { text: "", reasoning: "", tools: [] };
  let lastPaint = 0;

  const paint = (force = false) => {
    const now = performance.now();
    if (!force && now - lastPaint < 50) return;
    lastPaint = now;
    updateLiveAssistant(liveNode, live);
  };

  while (true) {
    const { done, value } = await reader.read();
    buf += decoder.decode(value || new Uint8Array(), { stream: !done });
    buf = buf.replaceAll("\r\n", "\n");
    let idx;
    while ((idx = buf.indexOf("\n\n")) >= 0) {
      const block = buf.slice(0, idx);
      buf = buf.slice(idx + 2);
      const dataLine = block
        .split("\n")
        .filter((l) => l.startsWith("data:"))
        .map((l) => l.slice(5).trimStart())
        .join("\n");
      if (!dataLine) continue;
      let ev;
      try { ev = JSON.parse(dataLine); } catch { continue; }
      if (ev.type === "delta") {
        live = ev.snapshot || live;
        paint(false);
      } else if (ev.type === "done") {
        live = ev.snapshot || live;
        paint(true);
        if (liveNode) {
          liveNode.classList.remove("streaming");
          liveNode.querySelector(".stream-dot")?.remove();
          delete liveNode.dataset.live;
          if (ev.assistant_id) liveNode.dataset.id = ev.assistant_id;
        }
        if (ev.user_id) {
          const box = liveNode?.parentElement;
          const tmpUser = box?.querySelector(".msg.user[data-id^='tmp-user-']");
          if (tmpUser) tmpUser.dataset.id = ev.user_id;
        }
        if (Array.isArray(ev.messages)) {
          updateLocalItemMessages(historyId, ev.messages);
        }
        return live;
      } else if (ev.type === "error") {
        throw new Error(ev.message || "stream error");
      }
    }
    if (done) break;
  }
  paint(true);
  return live;
}

function renderImage(item, data) {
  const toolbar = $("#toolbar");
  toolbar.innerHTML = `
    <label>模型<select id="img-model">${modelOptions((m) => /image/i.test(m.capability + m.id) && !/edit/i.test(m.id))}</select></label>`;
  const modelSel = $("#img-model");
  if (data.model) {
    if (![...modelSel.options].some((o) => o.value === data.model)) {
      modelSel.add(new Option(data.model, data.model, true, true));
    }
    modelSel.value = data.model;
  }

  const content = $("#content");
  const form = el("div", "form-grid");
  form.innerHTML = `
    <label>Prompt<textarea id="img-prompt" rows="4">${escapeHtml(data.prompt || "")}</textarea></label>
    <div class="row">
      <label>数量<input id="img-n" type="number" min="1" max="4" value="${data.count || 1}" /></label>
      <label>比例<select id="img-ar">${["1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3"].map((v) => `<option ${data.aspect_ratio === v ? "selected" : ""}>${v}</option>`).join("")}</select></label>
      <label>分辨率<select id="img-res">${["1k", "2k"].map((v) => `<option ${data.resolution === v ? "selected" : ""}>${v}</option>`).join("")}</select></label>
      <button id="img-go">生成</button>
    </div>
    <div id="img-status" class="status"></div>
    <div class="media-grid" id="img-grid"></div>`;
  content.appendChild(form);
  paintImages($("#img-grid"), data.images || []);
  $("#img-go").onclick = async () => {
    $("#img-status").textContent = "生成中…";
    try {
      const res = await api("/api/images/generate", {
        method: "POST",
        body: JSON.stringify({
          history_id: item.id,
          model: $("#img-model").value,
          prompt: $("#img-prompt").value,
          count: Number($("#img-n").value || 1),
          aspect_ratio: $("#img-ar").value,
          resolution: $("#img-res").value,
        }),
      });
      paintImages($("#img-grid"), res.images || []);
      $("#img-status").textContent = "完成";
      await refreshHistory();
    } catch (e) {
      $("#img-status").textContent = e.message;
      $("#img-status").classList.add("error");
    }
  };
}

function renderImageEdit(item, data) {
  const toolbar = $("#toolbar");
  toolbar.innerHTML = `
    <label>模型<select id="edit-model">${modelOptions((m) => /edit|image/i.test(m.capability + m.id))}</select></label>`;
  const modelSel = $("#edit-model");
  const preferred = data.model || "grok-imagine-image-edit";
  if (![...modelSel.options].some((o) => o.value === preferred)) {
    modelSel.add(new Option(preferred, preferred, true, true));
  }
  modelSel.value = preferred;

  const content = $("#content");
  const form = el("div", "form-grid");
  form.innerHTML = `
    <label>源图片 URL 或 data URL<textarea id="edit-src" rows="2">${escapeHtml(data.source_url || "")}</textarea></label>
    <label>或上传本地图片<input id="edit-file" type="file" accept="image/*" /></label>
    <label>编辑说明<textarea id="edit-prompt" rows="3">${escapeHtml(data.prompt || "")}</textarea></label>
    <div class="row">
      <label>数量<input id="edit-n" type="number" min="1" max="4" value="${data.count || 1}" /></label>
      <label>比例<select id="edit-ar">${["1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3"].map((v) => `<option ${data.aspect_ratio === v ? "selected" : ""}>${v}</option>`).join("")}</select></label>
      <label>分辨率<select id="edit-res">${["1k", "2k"].map((v) => `<option ${data.resolution === v ? "selected" : ""}>${v}</option>`).join("")}</select></label>
      <button id="edit-go">编辑生成</button>
    </div>
    <div id="edit-status" class="status"></div>
    <div class="media-grid" id="edit-grid"></div>`;
  content.appendChild(form);
  paintImages($("#edit-grid"), data.images || []);

  $("#edit-file").onchange = async (e) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const b64 = await fileToDataUrl(file);
    const up = await api("/api/media/upload", {
      method: "POST",
      body: JSON.stringify({ data: b64, filename: file.name }),
    });
    $("#edit-src").value = up.data_url || up.local_url;
  };

  $("#edit-go").onclick = async () => {
    $("#edit-status").textContent = "处理中…";
    try {
      const res = await api("/api/images/edit", {
        method: "POST",
        body: JSON.stringify({
          history_id: item.id,
          model: $("#edit-model").value,
          prompt: $("#edit-prompt").value,
          image_url: $("#edit-src").value,
          count: Number($("#edit-n").value || 1),
          aspect_ratio: $("#edit-ar").value,
          resolution: $("#edit-res").value,
        }),
      });
      paintImages($("#edit-grid"), res.images || []);
      $("#edit-status").textContent = "完成";
      await refreshHistory();
    } catch (e) {
      $("#edit-status").textContent = e.message;
      $("#edit-status").classList.add("error");
    }
  };
}

function renderVideo(item, data) {
  const toolbar = $("#toolbar");
  toolbar.innerHTML = `
    <label>模型<select id="vid-model">${modelOptions((m) => /video/i.test(m.capability + m.id))}</select></label>`;
  const modelSel = $("#vid-model");
  const preferred = data.model || "grok-imagine-video";
  if (![...modelSel.options].some((o) => o.value === preferred)) {
    modelSel.add(new Option(preferred, preferred, true, true));
  }
  modelSel.value = preferred;

  const content = $("#content");
  const form = el("div", "form-grid");
  form.innerHTML = `
    <label>Prompt<textarea id="vid-prompt" rows="3">${escapeHtml(data.prompt || "")}</textarea></label>
    <label>参考图 URL（可选）<input id="vid-img" type="text" value="${escapeAttr(data.image_url || "")}" /></label>
    <div class="row">
      <label>时长<select id="vid-dur">${[6, 10, 15].map((v) => `<option value="${v}" ${Number(data.duration) === v ? "selected" : ""}>${v}s</option>`).join("")}</select></label>
      <label>比例<select id="vid-ar">${["1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3"].map((v) => `<option ${data.aspect_ratio === v ? "selected" : ""}>${v}</option>`).join("")}</select></label>
      <label>分辨率<select id="vid-res">${["480p", "720p", "1080p"].map((v) => `<option ${data.resolution === v ? "selected" : ""}>${v}</option>`).join("")}</select></label>
      <button id="vid-go">生成视频</button>
    </div>
    <div id="vid-status" class="status">${escapeHtml(data.status || "idle")} ${data.progress ? data.progress + "%" : ""}</div>
    <div class="progress"><span id="vid-bar" style="width:${data.progress || 0}%"></span></div>
    <div class="media-grid" id="vid-grid"></div>`;
  content.appendChild(form);
  if (data.local_path || data.video_url) {
    paintVideo($("#vid-grid"), data);
  }
  $("#vid-go").onclick = async () => {
    $("#vid-status").textContent = "提交任务…";
    try {
      const res = await api("/api/videos/create", {
        method: "POST",
        body: JSON.stringify({
          history_id: item.id,
          model: $("#vid-model").value,
          prompt: $("#vid-prompt").value,
          image_url: $("#vid-img").value || null,
          duration: Number($("#vid-dur").value),
          aspect_ratio: $("#vid-ar").value,
          resolution: $("#vid-res").value,
        }),
      });
      await pollVideo(res.request_id);
      await refreshHistory();
    } catch (e) {
      $("#vid-status").textContent = e.message;
      $("#vid-status").classList.add("error");
    }
  };
  if (data.request_id && data.status === "pending") {
    pollVideo(data.request_id);
  }
}

async function pollVideo(requestId) {
  for (;;) {
    const st = await api(`/api/videos/${encodeURIComponent(requestId)}`);
    $("#vid-status").textContent = `${st.status} ${st.progress || 0}%`;
    const bar = $("#vid-bar");
    if (bar) bar.style.width = `${st.progress || 0}%`;
    if (st.status === "done") {
      paintVideo($("#vid-grid"), {
        video_url: st.video_url,
        local_path: st.local_path,
        local_url: st.local_url,
      });
      return;
    }
    if (st.status === "failed") {
      throw new Error(st.error || "video failed");
    }
    await new Promise((r) => setTimeout(r, 3000));
  }
}

function paintImages(grid, images) {
  if (!grid) return;
  grid.innerHTML = "";
  for (const img of images) {
    const card = el("div", "media-card");
    const image = el("img");
    image.src = mediaUrl(img);
    image.alt = "generated";
    card.appendChild(image);
    card.appendChild(el("div", "cap", img.revised_prompt || img.url || img.local_path || ""));
    grid.appendChild(card);
  }
}

function paintVideo(grid, data) {
  if (!grid) return;
  grid.innerHTML = "";
  const card = el("div", "media-card");
  const video = document.createElement("video");
  video.controls = true;
  video.src = data.local_url || mediaUrl({ local_path: data.local_path, url: data.video_url });
  card.appendChild(video);
  card.appendChild(el("div", "cap", data.video_url || data.local_path || ""));
  grid.appendChild(card);
}

function fileToDataUrl(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = reject;
    reader.readAsDataURL(file);
  });
}

async function createItem(kind) {
  const item = await api("/api/history", {
    method: "POST",
    body: JSON.stringify({ kind }),
  });
  state.activeId = item.id;
  await refreshHistory();
  renderMain(state.items.find((x) => x.id === item.id) || item);
}

function bindUi() {
  $$("[data-new]").forEach((btn) => {
    btn.onclick = () => createItem(btn.dataset.new);
  });
  $("#history-filter").oninput = (e) => {
    state.filter = e.target.value;
    renderHistory();
  };
  $("#btn-settings").onclick = () => $("#settings-dialog").showModal();
  $("#btn-test").onclick = async () => {
    $("#cfg-status").textContent = "测试中…";
    try {
      await api("/api/config", {
        method: "PUT",
        body: JSON.stringify({
          base_url: $("#cfg-base").value,
          api_key: $("#cfg-key").value || undefined,
        }),
      });
      await refreshConfig();
      await refreshModels();
      $("#cfg-status").textContent = `OK，模型 ${state.models.length} 个`;
    } catch (e) {
      $("#cfg-status").textContent = e.message;
    }
  };
  $("#settings-form").onsubmit = async (e) => {
    if (e.submitter?.value === "cancel") return;
    e.preventDefault();
    await api("/api/config", {
      method: "PUT",
      body: JSON.stringify({
        base_url: $("#cfg-base").value,
        api_key: $("#cfg-key").value || undefined,
        default_chat_model: $("#cfg-chat-model").value,
        default_image_model: $("#cfg-image-model").value,
        default_image_edit_model: $("#cfg-edit-model").value,
        default_video_model: $("#cfg-video-model").value,
      }),
    });
    await refreshConfig();
    try { await refreshModels(); } catch { /* ignore */ }
    $("#settings-dialog").close();
  };
}

async function boot() {
  setupMarkdown();
  bindUi();
  await refreshConfig();
  if (state.config.ready) {
    try { await refreshModels(); } catch { /* ignore */ }
  } else {
    $("#settings-dialog").showModal();
  }
  await refreshHistory();
}

boot().catch((e) => {
  console.error(e);
  alert(e.message || String(e));
});
