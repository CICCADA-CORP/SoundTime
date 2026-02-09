import Database from "better-sqlite3";
import { fileURLToPath } from "url";
import { dirname, join } from "path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const DB_PATH = process.env.DB_PATH || join(__dirname, "data", "listing.db");

// Ensure data directory exists
import { mkdirSync } from "fs";
mkdirSync(dirname(DB_PATH), { recursive: true });

const db = new Database(DB_PATH);

// Enable WAL mode for better concurrent read performance
db.pragma("journal_mode = WAL");
db.pragma("foreign_keys = ON");

// Create tables
db.exec(`
  CREATE TABLE IF NOT EXISTS nodes (
    id            TEXT PRIMARY KEY,
    domain        TEXT UNIQUE NOT NULL,
    name          TEXT NOT NULL DEFAULT '',
    description   TEXT NOT NULL DEFAULT '',
    version       TEXT NOT NULL DEFAULT '',
    track_count   INTEGER NOT NULL DEFAULT 0,
    user_count    INTEGER NOT NULL DEFAULT 0,
    open_registration INTEGER NOT NULL DEFAULT 1,
    p2p_enabled   INTEGER NOT NULL DEFAULT 0,
    p2p_node_id   TEXT,
    country       TEXT,
    token         TEXT UNIQUE NOT NULL,
    first_seen    TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen     TEXT NOT NULL DEFAULT (datetime('now')),
    last_healthy  TEXT NOT NULL DEFAULT (datetime('now')),
    is_online     INTEGER NOT NULL DEFAULT 1,
    down_since    TEXT
  );

  CREATE INDEX IF NOT EXISTS idx_nodes_online ON nodes(is_online);
  CREATE INDEX IF NOT EXISTS idx_nodes_domain ON nodes(domain);
`);

export default db;
