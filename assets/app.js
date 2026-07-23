const state = {
  config: null,
  models: [],
  items: [],
  activeId: null,
  filter: "",
  streaming: false,
};

const $ = (sel) => document.querySelector(sel);
const el = (tag, cls, text) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
};

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

function activeItem() {
  return state.items.find((x) => x.id === state.activeId) || null;
}

function chatPayload(item) {
  return item?.payload?.payload_type === "chat" ? item.payload : item?.payload;
}
function isChat(item) { return item?.kind === "chat"; }
function isImage(item) { return item?.kind === "image"; }
function isEdit(item) { return item?.kind === "image_edit"; }
function isVideo(item) { return item?.kind === "video"; }

function payloadOf(item) {
  if (!item) return null;
  const p = item.payload;
  if (!p) return null;
  // serde: { payload_type, data }
  if (p.payload_type && p.data) return { type: p.payload_type, data: p.data };
  if (p.messages) return { type: "chat", data: p };
  if (item.kind === "image") return { type: "image", data: p };
  if (item.kind === "image_edit") return { type: "image_edit", data: p };
  if (item.kind === "video") return { type: "video", data: p };
  return { type: item.kind, data: p };
}

async function refreshConfig() {
  state.config = await api("/api/config");
  const banner = $("#setup-banner");
  if (!state.config.ready) banner.classList.remove("hidden");
  else banner.classList.add("hidden");
  $("#cfg-base").value = state.config.base_url || "";
  $("#cfg-key").value = "";
  $("#cfg-masked").textContent = state.config.api_key_set ? `当前 Key: ${state.config.api_key_masked}` : "尚未设置 Key";
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

async function refreshHistory() {
  const data = await api("/api/history");
  state.items = data.items || [];
  renderHistory();
  if (state.activeId) {
    const still = state.items.find((x) => x.id === state.activeId);
    if (still) renderMain(still);
    else {
      state.activeId = null;
      renderMain(null);
    }
  }
}

function renderHistory() {
  const list = $("#history-list");
  list.innerHTML = "";
  const q = state.filter.trim().toLowerCase();
  const items = state.items.filter((it) => !q || (it.title || "").toLowerCase().includes(q) || it.kind.includes(q));
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
  return models.map((m) => `<option value="${escapeAttr(m.id)}">${escapeHtml(m.id)}</option>`).join("");
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}
function escapeAttr(s) { return escapeHtml(s); }

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
  else {
    content.textContent = "未知类型";
  }
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
        ${["auto","none","low","medium","high","xhigh"].map((v) =>
          `<option value="${v}" ${chat.reasoning_effort === v ? "selected" : ""}>${v}</option>`).join("")}
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
  box.scrollTop = box.scrollHeight;

  const composer = $("#composer");
  composer.classList.remove("hidden");
  composer.innerHTML = `
    <textarea id="chat-input" placeholder="输入消息… Enter 发送，Shift+Enter 换行"></textarea>
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

function renderMessage(item, msg) {
  const node = el("div", `msg ${msg.role}`);
  node.dataset.id = msg.id;
  const role = el("div", "role", msg.role);
  const body = el("div", "body", msg.content || "");
  node.appendChild(role);
  node.appendChild(body);
  if (msg.reasoning) {
    const r = el("div", "reasoning");
    r.textContent = msg.reasoning;
    node.appendChild(r);
  }
  if (msg.tools?.length) {
    const tools = el("div", "tools");
    for (const t of msg.tools) {
      const row = el("div", "tool");
      row.textContent = `${t.status} · ${t.name || t.type} ${t.detail ? "· " + t.detail : ""}`;
      tools.appendChild(row);
    }
    node.appendChild(tools);
  }
  const actions = el("div", "msg-actions");
  if (msg.role === "user" || msg.role === "assistant") {
    const editBtn = el("button", null, "编辑");
    editBtn.onclick = async () => {
      const next = prompt("编辑消息内容", msg.content || "");
      if (next == null) return;
      const resend = msg.role === "user" ? confirm("是否用新内容重新生成回复？") : false;
      await api(`/api/chat/${item.id}/edit-message`, {
        method: "POST",
        body: JSON.stringify({ message_id: msg.id, content: next, resend }),
      });
      await refreshHistory();
      if (resend && msg.role === "user") {
        await streamRetry(item.id, msg.id);
      }
    };
    actions.appendChild(editBtn);
  }
  const delBtn = el("button", null, "删除");
  delBtn.onclick = async () => {
    if (!confirm("删除该消息？")) return;
    await api(`/api/chat/${item.id}/delete-message`, {
      method: "POST",
      body: JSON.stringify({ message_id: msg.id }),
    });
    await refreshHistory();
  };
  actions.appendChild(delBtn);
  if (msg.role === "user") {
    const retryBtn = el("button", null, "重试");
    retryBtn.onclick = () => streamRetry(item.id, msg.id);
    actions.appendChild(retryBtn);
  }
  node.appendChild(actions);
  return node;
}

async function sendChat(item) {
  if (state.streaming) return;
  const text = $("#chat-input")?.value?.trim();
  if (!text) return;
  // save settings first
  const chat = payloadOf(item).data;
  chat.model = $("#chat-model")?.value || chat.model;
  chat.web_search = $("#chat-web")?.checked ?? chat.web_search;
  chat.x_search = $("#chat-x")?.checked ?? chat.x_search;
  chat.reasoning_effort = $("#chat-reason")?.value || chat.reasoning_effort;
  item.payload = { payload_type: "chat", data: chat };
  await api(`/api/history/${item.id}`, { method: "PUT", body: JSON.stringify(item) });

  state.streaming = true;
  $("#chat-status").textContent = "生成中…";
  $("#chat-input").value = "";
  try {
    await consumeSse("/api/chat/stream", { history_id: item.id, content: text }, item.id);
  } catch (e) {
    $("#chat-status").textContent = e.message;
    $("#chat-status").classList.add("error");
  } finally {
    state.streaming = false;
    await refreshHistory();
  }
}

async function streamRetry(historyId, messageId) {
  if (state.streaming) return;
  state.streaming = true;
  try {
    await consumeSse(`/api/chat/${historyId}/retry`, { message_id: messageId }, historyId);
  } catch (e) {
    alert(e.message);
  } finally {
    state.streaming = false;
    await refreshHistory();
  }
}

async function consumeSse(url, body, historyId) {
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

  // ensure UI shows streaming assistant bubble
  const ensureLive = () => {
    const content = $("#content .messages");
    if (!content) return null;
    let node = content.querySelector(".msg.assistant.live");
    if (!node) {
      node = el("div", "msg assistant live");
      node.innerHTML = `<div class="role">assistant</div><div class="body"></div><div class="reasoning hidden"></div><div class="tools"></div>`;
      content.appendChild(node);
    }
    return node;
  };

  while (true) {
    const { done, value } = await reader.read();
    buf += decoder.decode(value || new Uint8Array(), { stream: !done });
    buf = buf.replaceAll("\r\n", "\n");
    let idx;
    while ((idx = buf.indexOf("\n\n")) >= 0) {
      const block = buf.slice(0, idx);
      buf = buf.slice(idx + 2);
      const dataLine = block.split("\n").filter((l) => l.startsWith("data:")).map((l) => l.slice(5).trimStart()).join("\n");
      if (!dataLine) continue;
      let ev;
      try { ev = JSON.parse(dataLine); } catch { continue; }
      if (ev.type === "delta" || ev.type === "done") {
        live = ev.snapshot || live;
        const node = ensureLive();
        if (node) {
          node.querySelector(".body").textContent = live.text || "";
          const r = node.querySelector(".reasoning");
          if (live.reasoning) {
            r.classList.remove("hidden");
            r.textContent = live.reasoning;
          }
          const tools = node.querySelector(".tools");
          tools.innerHTML = "";
          for (const t of live.tools || []) {
            tools.appendChild(el("div", "tool", `${t.status} · ${t.name || t.type_name || t.type} ${t.detail || ""}`));
          }
          node.parentElement.scrollTop = node.parentElement.scrollHeight;
        }
        if (ev.type === "done") return;
      } else if (ev.type === "error") {
        throw new Error(ev.message || "stream error");
      }
    }
    if (done) break;
  }
}

function renderImage(item, data) {
  const toolbar = $("#toolbar");
  toolbar.innerHTML = `
    <label>模型<select id="img-model">${modelOptions((m) => /image/i.test(m.capability + m.id) && !/edit/i.test(m.id))}</select></label>`;
  const modelSel = $("#img-model");
  if (data.model) {
    if (![...modelSel.options].some((o) => o.value === data.model)) modelSel.add(new Option(data.model, data.model, true, true));
    modelSel.value = data.model;
  }

  const content = $("#content");
  const form = el("div", "form-grid");
  form.innerHTML = `
    <label>Prompt<textarea id="img-prompt" rows="4">${escapeHtml(data.prompt || "")}</textarea></label>
    <div class="row">
      <label>数量<input id="img-n" type="number" min="1" max="4" value="${data.count || 1}" /></label>
      <label>比例<select id="img-ar">${["1:1","16:9","9:16","4:3","3:4","3:2","2:3"].map((v)=>`<option ${data.aspect_ratio===v?"selected":""}>${v}</option>`).join("")}</select></label>
      <label>分辨率<select id="img-res">${["1k","2k"].map((v)=>`<option ${data.resolution===v?"selected":""}>${v}</option>`).join("")}</select></label>
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
  if (![...modelSel.options].some((o) => o.value === preferred)) modelSel.add(new Option(preferred, preferred, true, true));
  modelSel.value = preferred;

  const content = $("#content");
  const form = el("div", "form-grid");
  form.innerHTML = `
    <label>源图片 URL 或 data URL<textarea id="edit-src" rows="2">${escapeHtml(data.source_url || "")}</textarea></label>
    <label>或上传本地图片<input id="edit-file" type="file" accept="image/*" /></label>
    <label>编辑说明<textarea id="edit-prompt" rows="3">${escapeHtml(data.prompt || "")}</textarea></label>
    <div class="row">
      <label>数量<input id="edit-n" type="number" min="1" max="4" value="${data.count || 1}" /></label>
      <label>比例<select id="edit-ar">${["1:1","16:9","9:16","4:3","3:4","3:2","2:3"].map((v)=>`<option ${data.aspect_ratio===v?"selected":""}>${v}</option>`).join("")}</select></label>
      <label>分辨率<select id="edit-res">${["1k","2k"].map((v)=>`<option ${data.resolution===v?"selected":""}>${v}</option>`).join("")}</select></label>
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
    const up = await api("/api/media/upload", { method: "POST", body: JSON.stringify({ data: b64, filename: file.name }) });
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
  if (![...modelSel.options].some((o) => o.value === preferred)) modelSel.add(new Option(preferred, preferred, true, true));
  modelSel.value = preferred;

  const content = $("#content");
  const form = el("div", "form-grid");
  form.innerHTML = `
    <label>Prompt<textarea id="vid-prompt" rows="3">${escapeHtml(data.prompt || "")}</textarea></label>
    <label>参考图 URL（可选）<input id="vid-img" type="text" value="${escapeAttr(data.image_url || "")}" /></label>
    <div class="row">
      <label>时长<select id="vid-dur">${[6,10,15].map((v)=>`<option value="${v}" ${Number(data.duration)===v?"selected":""}>${v}s</option>`).join("")}</select></label>
      <label>比例<select id="vid-ar">${["1:1","16:9","9:16","4:3","3:4","3:2","2:3"].map((v)=>`<option ${data.aspect_ratio===v?"selected":""}>${v}</option>`).join("")}</select></label>
      <label>分辨率<select id="vid-res">${["480p","720p","1080p"].map((v)=>`<option ${data.resolution===v?"selected":""}>${v}</option>`).join("")}</select></label>
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
    const cap = el("div", "cap", img.revised_prompt || img.url || img.local_path || "");
    card.appendChild(cap);
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
  document.querySelectorAll("[data-new]").forEach((btn) => {
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
    try { await refreshModels(); } catch {}
    $("#settings-dialog").close();
  };
}

async function boot() {
  bindUi();
  await refreshConfig();
  if (state.config.ready) {
    try { await refreshModels(); } catch {}
  } else {
    $("#settings-dialog").showModal();
  }
  await refreshHistory();
}

boot().catch((e) => {
  console.error(e);
  alert(e.message || String(e));
});
