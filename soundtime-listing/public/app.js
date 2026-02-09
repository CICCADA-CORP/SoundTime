// ── SoundTime Listing — Frontend ──────────────────────────────────

const API = "";

let allNodes = [];

// ── Formatting helpers ───────────────────────────────────────────
function formatNumber(n) {
  if (n == null) return "0";
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "K";
  return String(n);
}

function timeAgo(dateStr) {
  if (!dateStr) return "unknown";
  const date = new Date(dateStr.endsWith("Z") ? dateStr : dateStr + "Z");
  const diff = Date.now() - date.getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

function downDuration(downSince) {
  if (!downSince) return "";
  const date = new Date(downSince.endsWith("Z") ? downSince : downSince + "Z");
  const diff = Date.now() - date.getTime();
  const hours = Math.floor(diff / 3600000);
  if (hours < 1) return "< 1h";
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  return `${days}d ${hours % 24}h`;
}

// ── Render a single node card ────────────────────────────────────
function renderNode(node) {
  const isOnline = node.is_online;
  const displayName = node.name || node.domain;

  const tags = [];
  if (node.version) tags.push(`<span class="tag version">v${node.version}</span>`);
  if (node.open_registration) {
    tags.push(`<span class="tag open">Open</span>`);
  } else {
    tags.push(`<span class="tag closed">Invite-only</span>`);
  }
  if (node.p2p_enabled) tags.push(`<span class="tag p2p">P2P</span>`);

  const proto = node.domain.includes("localhost") || node.domain.includes("127.0.0.1")
    ? "http" : "https";

  return `
    <div class="node-card ${isOnline ? "" : "offline"}">
      <div class="node-header">
        <div>
          <div class="node-name">${escapeHtml(displayName)}</div>
          <div class="node-domain">${escapeHtml(node.domain)}</div>
        </div>
        <div class="status-dot ${isOnline ? "online" : "offline"}" title="${isOnline ? "Online" : "Offline"}"></div>
      </div>

      ${node.description ? `<div class="node-description">${escapeHtml(node.description)}</div>` : ""}

      <div class="node-tags">${tags.join("")}</div>

      <div class="node-stats">
        <div class="node-stat"><strong>${formatNumber(node.track_count)}</strong> tracks</div>
        <div class="node-stat"><strong>${formatNumber(node.user_count)}</strong> users</div>
      </div>

      <div class="node-footer">
        <div>
          ${isOnline
            ? `<span>Last seen: ${timeAgo(node.last_seen)}</span>`
            : `<span class="down-info">Down for ${downDuration(node.down_since)}</span>`
          }
        </div>
        <a href="${proto}://${escapeHtml(node.domain)}" class="visit-btn" target="_blank" rel="noopener">
          Visit →
        </a>
      </div>
    </div>
  `;
}

function escapeHtml(str) {
  if (!str) return "";
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// ── Filtering ────────────────────────────────────────────────────
function getFilteredNodes() {
  const search = document.getElementById("search-input").value.toLowerCase().trim();
  const openOnly = document.getElementById("filter-open").checked;
  const p2pOnly = document.getElementById("filter-p2p").checked;
  const showOffline = document.getElementById("filter-offline").checked;

  return allNodes.filter((node) => {
    if (!showOffline && !node.is_online) return false;
    if (openOnly && !node.open_registration) return false;
    if (p2pOnly && !node.p2p_enabled) return false;
    if (search) {
      const s = `${node.name} ${node.domain} ${node.description}`.toLowerCase();
      if (!s.includes(search)) return false;
    }
    return true;
  });
}

function renderNodes() {
  const filtered = getFilteredNodes();
  const container = document.getElementById("nodes-list");
  const empty = document.getElementById("empty-state");

  if (filtered.length === 0) {
    container.innerHTML = "";
    empty.style.display = "block";
  } else {
    empty.style.display = "none";
    container.innerHTML = filtered.map(renderNode).join("");
  }
}

// ── Data Fetching ────────────────────────────────────────────────
async function loadNodes() {
  try {
    const res = await fetch(`${API}/api/nodes?include_offline=true`);
    const data = await res.json();
    allNodes = data.nodes || [];
    renderNodes();
  } catch (err) {
    console.error("Failed to load nodes:", err);
    document.getElementById("nodes-list").innerHTML =
      `<div class="loading">Failed to load nodes. Please try again later.</div>`;
  }
}

async function loadStats() {
  try {
    const res = await fetch(`${API}/api/stats`);
    const stats = await res.json();
    document.getElementById("stat-nodes").textContent = formatNumber(stats.online_nodes || 0);
    document.getElementById("stat-tracks").textContent = formatNumber(stats.total_tracks || 0);
    document.getElementById("stat-users").textContent = formatNumber(stats.total_users || 0);
  } catch (err) {
    console.error("Failed to load stats:", err);
  }
}

// ── Event Listeners ──────────────────────────────────────────────
document.getElementById("search-input").addEventListener("input", renderNodes);
document.getElementById("filter-open").addEventListener("change", renderNodes);
document.getElementById("filter-p2p").addEventListener("change", renderNodes);
document.getElementById("filter-offline").addEventListener("change", renderNodes);

// ── Init ─────────────────────────────────────────────────────────
loadStats();
loadNodes();

// Auto-refresh every 60 seconds
setInterval(() => {
  loadStats();
  loadNodes();
}, 60_000);
