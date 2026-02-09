import express from "express";
import cors from "cors";
import crypto from "crypto";
import { fileURLToPath } from "url";
import { dirname, join } from "path";
import db from "./db.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const app = express();
const PORT = process.env.PORT || 3333;

// ── Middleware ─────────────────────────────────────────────────────
app.use(cors());
app.use(express.json());
app.use(express.static(join(__dirname, "public")));

// ── Health Check Interval (every 5 minutes) ───────────────────────
const HEALTH_CHECK_INTERVAL_MS = 5 * 60 * 1000;
const REMOVAL_THRESHOLD_HOURS = 48;

// ── Helpers ──────────────────────────────────────────────────────
function generateToken() {
  return crypto.randomBytes(32).toString("hex");
}

function generateId() {
  return crypto.randomUUID();
}

/** Check if a SoundTime instance is reachable */
async function checkNodeHealth(domain) {
  const protocols = domain.startsWith("localhost") || domain.includes("127.0.0.1")
    ? ["http"]
    : ["https", "http"];

  for (const proto of protocols) {
    try {
      const url = `${proto}://${domain}/healthz`;
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 8000);

      const res = await fetch(url, {
        signal: controller.signal,
        headers: { "User-Agent": "SoundTime-Listing/1.0" },
      });
      clearTimeout(timeout);

      if (res.ok) {
        const body = await res.json().catch(() => null);
        if (body && body.status === "ok") return true;
      }
    } catch {
      // try next protocol
    }
  }
  return false;
}

/** Fetch instance stats from a healthy node */
async function fetchNodeInfo(domain) {
  const protocols = domain.startsWith("localhost") || domain.includes("127.0.0.1")
    ? ["http"]
    : ["https", "http"];

  for (const proto of protocols) {
    try {
      const url = `${proto}://${domain}/api/nodeinfo`;
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 8000);

      const res = await fetch(url, {
        signal: controller.signal,
        headers: { "User-Agent": "SoundTime-Listing/1.0" },
      });
      clearTimeout(timeout);

      if (res.ok) {
        return await res.json();
      }
    } catch {
      // try next protocol
    }
  }
  return null;
}

// ── API Routes ───────────────────────────────────────────────────

/**
 * POST /api/announce — Node self-registration / heartbeat
 *
 * Body: { domain, name?, description?, version?, token? }
 *
 * - If no token: create a new registration, return the token (save it!)
 * - If token provided: update existing registration (heartbeat)
 */
app.post("/api/announce", async (req, res) => {
  const { domain, name, description, version, token } = req.body;

  if (!domain || typeof domain !== "string") {
    return res.status(400).json({ error: "domain is required" });
  }

  const cleanDomain = domain.replace(/^https?:\/\//, "").replace(/\/$/, "");

  try {
    if (token) {
      // Heartbeat: update existing node
      const node = db.prepare("SELECT * FROM nodes WHERE token = ?").get(token);

      if (!node) {
        return res.status(401).json({ error: "invalid token" });
      }

      // Verify the domain matches the token
      if (node.domain !== cleanDomain) {
        return res.status(403).json({ error: "token does not match domain" });
      }

      // Try to fetch fresh stats from the node
      const info = await fetchNodeInfo(cleanDomain);

      const updateStmt = db.prepare(`
        UPDATE nodes SET
          name = ?,
          description = ?,
          version = ?,
          track_count = ?,
          user_count = ?,
          open_registration = ?,
          p2p_enabled = ?,
          p2p_node_id = ?,
          last_seen = datetime('now'),
          last_healthy = datetime('now'),
          is_online = 1,
          down_since = NULL
        WHERE token = ?
      `);

      updateStmt.run(
        name || info?.name || node.name || "",
        description || info?.description || node.description || "",
        version || info?.version || node.version || "",
        info?.track_count ?? node.track_count ?? 0,
        info?.user_count ?? node.user_count ?? 0,
        info?.open_registration ?? node.open_registration ?? 1,
        info?.p2p_enabled ? 1 : 0,
        info?.p2p_node_id || node.p2p_node_id || null,
        token
      );

      return res.json({
        status: "updated",
        id: node.id,
        domain: cleanDomain,
      });
    }

    // New registration
    const existing = db.prepare("SELECT * FROM nodes WHERE domain = ?").get(cleanDomain);
    if (existing) {
      return res.status(409).json({
        error: "domain already registered",
        hint: "Include the token from your initial registration to update.",
      });
    }

    // Verify the node is reachable before accepting
    const isHealthy = await checkNodeHealth(cleanDomain);
    if (!isHealthy) {
      return res.status(422).json({
        error: "node is not reachable",
        hint: `Could not reach ${cleanDomain}. Ensure your instance is online and accessible.`,
      });
    }

    // Fetch initial info
    const info = await fetchNodeInfo(cleanDomain);

    const newToken = generateToken();
    const id = generateId();

    db.prepare(`
      INSERT INTO nodes (id, domain, name, description, version, track_count, user_count,
                         open_registration, p2p_enabled, p2p_node_id, token)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `).run(
      id,
      cleanDomain,
      name || info?.name || "",
      description || info?.description || "",
      version || info?.version || "",
      info?.track_count ?? 0,
      info?.user_count ?? 0,
      info?.open_registration ?? 1,
      info?.p2p_enabled ? 1 : 0,
      info?.p2p_node_id || null,
      newToken
    );

    return res.status(201).json({
      status: "registered",
      id,
      domain: cleanDomain,
      token: newToken,
      message: "Save this token! You need it for future heartbeats and to remove your node.",
    });
  } catch (err) {
    console.error("announce error:", err);
    return res.status(500).json({ error: "internal server error" });
  }
});

/**
 * DELETE /api/nodes/:domain — Unregister a node
 * Header: Authorization: Bearer <token>
 */
app.delete("/api/nodes/:domain", (req, res) => {
  const token = req.headers.authorization?.replace("Bearer ", "");
  if (!token) {
    return res.status(401).json({ error: "token required" });
  }

  const cleanDomain = req.params.domain.replace(/^https?:\/\//, "").replace(/\/$/, "");

  const node = db.prepare("SELECT * FROM nodes WHERE domain = ? AND token = ?")
    .get(cleanDomain, token);

  if (!node) {
    return res.status(404).json({ error: "node not found or invalid token" });
  }

  db.prepare("DELETE FROM nodes WHERE id = ?").run(node.id);

  return res.json({ status: "removed", domain: cleanDomain });
});

/**
 * GET /api/nodes — Public list of online nodes
 * Query params: ?include_offline=true to include recently-offline nodes
 */
app.get("/api/nodes", (_req, res) => {
  const includeOffline = _req.query.include_offline === "true";

  const query = includeOffline
    ? `SELECT id, domain, name, description, version, track_count, user_count,
              open_registration, p2p_enabled, p2p_node_id, country,
              first_seen, last_seen, last_healthy, is_online, down_since
       FROM nodes ORDER BY is_online DESC, track_count DESC`
    : `SELECT id, domain, name, description, version, track_count, user_count,
              open_registration, p2p_enabled, p2p_node_id, country,
              first_seen, last_seen, last_healthy, is_online, down_since
       FROM nodes WHERE is_online = 1 ORDER BY track_count DESC`;

  const nodes = db.prepare(query).all();

  return res.json({
    total: nodes.length,
    nodes: nodes.map((n) => ({
      ...n,
      open_registration: !!n.open_registration,
      p2p_enabled: !!n.p2p_enabled,
      is_online: !!n.is_online,
    })),
  });
});

/**
 * GET /api/nodes/:domain — Get info about a specific node
 */
app.get("/api/nodes/:domain", (req, res) => {
  const cleanDomain = req.params.domain.replace(/^https?:\/\//, "").replace(/\/$/, "");

  const node = db.prepare(
    `SELECT id, domain, name, description, version, track_count, user_count,
            open_registration, p2p_enabled, p2p_node_id, country,
            first_seen, last_seen, last_healthy, is_online, down_since
     FROM nodes WHERE domain = ?`
  ).get(cleanDomain);

  if (!node) {
    return res.status(404).json({ error: "node not found" });
  }

  return res.json({
    ...node,
    open_registration: !!node.open_registration,
    p2p_enabled: !!node.p2p_enabled,
    is_online: !!node.is_online,
  });
});

/**
 * GET /api/stats — Aggregate stats
 */
app.get("/api/stats", (_req, res) => {
  const stats = db.prepare(`
    SELECT
      COUNT(*) as total_nodes,
      SUM(CASE WHEN is_online = 1 THEN 1 ELSE 0 END) as online_nodes,
      SUM(track_count) as total_tracks,
      SUM(user_count) as total_users
    FROM nodes
  `).get();

  return res.json(stats);
});

// ── Health Checker Background Job ─────────────────────────────────

async function runHealthChecks() {
  const nodes = db.prepare("SELECT * FROM nodes").all();
  console.log(`[health] checking ${nodes.length} nodes...`);

  for (const node of nodes) {
    const isHealthy = await checkNodeHealth(node.domain);

    if (isHealthy) {
      // Also refresh stats
      const info = await fetchNodeInfo(node.domain);

      db.prepare(`
        UPDATE nodes SET
          is_online = 1,
          last_healthy = datetime('now'),
          last_seen = datetime('now'),
          down_since = NULL,
          track_count = COALESCE(?, track_count),
          user_count = COALESCE(?, user_count),
          version = COALESCE(?, version),
          p2p_enabled = COALESCE(?, p2p_enabled),
          p2p_node_id = COALESCE(?, p2p_node_id),
          name = COALESCE(?, name),
          open_registration = COALESCE(?, open_registration)
        WHERE id = ?
      `).run(
        info?.track_count ?? null,
        info?.user_count ?? null,
        info?.version || null,
        info?.p2p_enabled != null ? (info.p2p_enabled ? 1 : 0) : null,
        info?.p2p_node_id || null,
        info?.name || null,
        info?.open_registration ?? null,
        node.id
      );

      if (!node.is_online) {
        console.log(`[health] ✅ ${node.domain} is back online`);
      }
    } else {
      // Mark as offline
      if (node.is_online) {
        console.log(`[health] ❌ ${node.domain} went offline`);
        db.prepare(`
          UPDATE nodes SET is_online = 0, down_since = datetime('now')
          WHERE id = ?
        `).run(node.id);
      }

      // Check if down for > 48h → remove
      if (node.down_since) {
        const downSince = new Date(node.down_since + "Z").getTime();
        const hoursDown = (Date.now() - downSince) / (1000 * 60 * 60);

        if (hoursDown >= REMOVAL_THRESHOLD_HOURS) {
          console.log(
            `[health] 🗑️  removing ${node.domain} (down for ${Math.round(hoursDown)}h)`
          );
          db.prepare("DELETE FROM nodes WHERE id = ?").run(node.id);
        }
      }
    }
  }

  console.log(`[health] checks complete`);
}

// Run health checks every 5 minutes
setInterval(runHealthChecks, HEALTH_CHECK_INTERVAL_MS);
// Run once at startup (after 10s to let things settle)
setTimeout(runHealthChecks, 10_000);

// ── Fallback: serve index.html for SPA ────────────────────────────
app.get("*", (_req, res) => {
  res.sendFile(join(__dirname, "public", "index.html"));
});

// ── Start ─────────────────────────────────────────────────────────
app.listen(PORT, () => {
  console.log(`🌐 SoundTime Listing running on http://localhost:${PORT}`);
});
