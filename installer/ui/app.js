"use strict";

const el = (id) => document.getElementById(id);
const panes = ["connect", "choose", "check-view", "progress", "success", "exported", "closed"];
const stages = ["prepare", "download", "start", "verify"];
const steps = [...el("steps").children];
const form = el("config-form");
const settingsFields = el("settings");
const progressItems = [...el("progress-list").children];
const stored = {
  read(key) { try { return sessionStorage.getItem(key); } catch { return null; } },
  write(key, value) { try { sessionStorage.setItem(key, value); } catch { /* The tab can run without storage. */ } },
  remove(key) { try { sessionStorage.removeItem(key); } catch { /* No stored session. */ } }
};
const fromLauncher = new URLSearchParams(location.hash.slice(1)).get("session");
let token = fromLauncher || stored.read("soundtime.session");
if (fromLauncher) {
  if (fromLauncher !== stored.read("soundtime.session")) stored.remove("soundtime.plan");
  stored.write("soundtime.session", token);
  history.replaceState(null, "", location.pathname + location.search);
}
let defaults;
let machine;
let plan;
let currentPane;
let timer;
let hasSession = false;

function display(id, visible) { el(id).hidden = !visible; }
function message(id, value) {
  if (el(id).textContent !== value) el(id).textContent = value;
}
function notice(value = "") {
  message("notice", value);
  display("notice", Boolean(value));
}
function pane(name, step, allowClose = true) {
  const changed = currentPane !== name;
  currentPane = name;
  panes.forEach(id => display(id, id === name));
  steps.forEach((item, index) => {
    item.classList.toggle("done", index < step);
    if (index === step) item.setAttribute("aria-current", "step");
    else item.removeAttribute("aria-current");
    item.firstElementChild.textContent = index < step ? "✓" : String(index + 1);
  });
  display("exit", hasSession && allowClose);
  notice();
  if (changed && name !== "connect") el(name).querySelector("h1").focus();
}
async function request(path, body, wantArchive = false) {
  let response;
  try {
    response = await fetch("/api/" + path, {
      method: body === undefined ? "GET" : "POST",
      headers: body === undefined
        ? { Authorization: "Bearer " + token }
        : { Authorization: "Bearer " + token, "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
      credentials: "omit", cache: "no-store", redirect: "error"
    });
  } catch {
    throw new Error("Connexion au lanceur interrompue. Vérifiez qu’il est toujours ouvert sur la machine à installer.");
  }
  if (response.status === 401 || response.status === 403) {
    throw new Error("Cette session a expiré. Rouvrez le lien fourni par le lanceur SoundTime.");
  }
  if (response.ok && wantArchive) {
    if (!response.headers.get("Content-Type")?.startsWith("application/zip")) throw new Error("L’archive n’a pas pu être préparée.");
    return response.blob();
  }
  const data = await response.json().catch(() => null);
  if (!response.ok) throw new Error(data?.error || "Le lanceur n’a pas pu terminer cette opération.");
  if (!data) throw new Error("La réponse du lanceur est illisible. Rouvrez l’assistant pour reprendre.");
  return data;
}

function settings() {
  if (!remote() && !localCustom()) return { ...defaults };
  return {
    directory: el("directory").value.trim(),
    musicDirectory: el("storage").value === "local" ? el("music-directory").value.trim() : "",
    storage: storageSettings(),
    port: el("port").valueAsNumber,
    access: el("access").value,
    domain: el("domain").value.trim(),
    p2p: el("p2p").checked,
    peerPort: el("p2p").checked ? el("peer-port").valueAsNumber : defaults.peerPort
  };
}
function updateFields(event) {
  destinationFields(event);
  display("peer-field", el("p2p").checked);
  el("peer-port").disabled = !el("p2p").checked;
}
function fillFields(values, mode) {
  form.elements.mode.value = mode === "custom" ? "custom" : "quick";
  el("directory").value = values.directory;
  el("music-directory").value = values.musicDirectory || "";
  el("port").value = values.port;
  el("domain").value = values.domain;
  el("access").value = values.access;
  el("p2p").checked = values.p2p;
  el("peer-port").value = values.peerPort;
  updateFields();
}
form.addEventListener("change", updateFields);
function table(id, rows) {
  const entries = rows.map(([label, value]) => {
    const row = document.createElement("div");
    const dt = document.createElement("dt");
    const dd = document.createElement("dd");
    dt.textContent = label;
    dd.textContent = String(value);
    row.append(dt, dd);
    return row;
  });
  el(id).replaceChildren(...entries);
}
function review() {
  const rows = [
    ["Machine", targetName()], ["Configuration", plan.directory],
    ["Accès web", plan.access === "lan" ? (exporting() && el("target").value === "nas" ? "Réseau privé du NAS" : "Réseau privé après activation") : (remote() ? "Tunnel ou proxy privé" : "Cet appareil")],
    ["Port web", plan.port], ["Nom d’hôte", plan.domain],
    ["Partage P2P", plan.p2p ? "Activé · UDP " + plan.peerPort : "Désactivé"]
  ];
  if (remote()) rows.splice(1, 0, ["Méthode", exporting() ? "Projet Docker à importer" : "Connexion SSH"]);
  rows.push(["Stockage audio", plan.storage?.kind === "s3" ? "S3 · " + plan.storage.bucket + " · " + plan.storage.region : "Volumes Docker"]);
  if (plan.musicDirectory) rows.push(["Musique existante", plan.musicDirectory]);
  table("review-settings", rows);
}
async function check() {
  if (!form.checkValidity()) {
    if (el("settings").querySelector(":invalid")) el("settings-panel").open = true;
    pane("choose", 0);
    form.reportValidity();
    return;
  }
  plan = settings();
  savePlan();
  pane("check-view", 1, false);
  if (exporting()) { confirmExport(); return; }
  messageTitle("check-view", sshMode() ? "Vérification du serveur" : "Vérification de la machine");
  message("start-label", "Installer SoundTime");
  const summary = el("check-message");
  summary.classList.add("busy");
  message("check-message", sshMode() ? "Connexion SSH et vérification de la machine distante…" : "Vérification de Docker, du dossier et des ports…");
  el("checks").replaceChildren();
  ["install", "recheck", "review", "docker-guide"].forEach(id => display(id, false));
  el("change").disabled = true;
  try {
    const result = await request(sshMode() ? "remote/check" : "check", sshMode() ? remoteBody() : plan);
    for (const check of result.checks) {
      const row = document.createElement("li");
      const mark = document.createElement("span");
      mark.className = "marker " + (check.ok ? "pass" : "fail");
      mark.textContent = check.ok ? "✓" : "!";
      mark.setAttribute("aria-label", check.ok ? "Validé" : "À corriger");
      const copy = document.createElement("div");
      const label = document.createElement("strong");
      const detail = document.createElement("p");
      label.textContent = check.label;
      detail.textContent = check.detail;
      copy.append(label, detail);
      row.append(mark, copy);
      el("checks").append(row);
    }
    const dockerProblem = result.checks.some(item => !item.ok && /docker|compose|daemon/i.test(item.id + " " + item.label));
    display("docker-guide", dockerProblem);
    message("check-message", result.ok
      ? "Votre machine est prête. Vous pouvez lancer le téléchargement et l’installation."
      : "Un réglage demande votre attention. Suivez les indications, puis relancez la vérification.");
    display("install", result.ok);
    display("recheck", !result.ok);
    review();
    display("review", true);
  } catch (error) {
    message("check-message", "La vérification n’a pas abouti.");
    notice(error.message);
    display("recheck", true);
  } finally {
    summary.classList.remove("busy");
    el("change").disabled = false;
    display("exit", true);
  }
}
form.addEventListener("submit", event => { event.preventDefault(); void check(); });
el("change").addEventListener("click", () => pane("choose", 0));
el("recheck").addEventListener("click", check);
el("recover").addEventListener("click", check);

function showEvents(events) {
  const list = el("events");
  if (list.children.length === events.length
      && events.every((entry, i) => list.children[i].textContent === entry.message)) return;
  list.replaceChildren(...events.map(entry => {
    const row = document.createElement("li");
    row.textContent = entry.message;
    return row;
  }));
}
function progress(status) {
  const failed = status.state === "failed";
  messageTitle("progress", failed ? "Installation interrompue" : "Installation en cours");
  pane("progress", 2, failed);
  message("progress-message", status.message || "Le lanceur prépare l’installation.");
  const index = stages.indexOf(status.stage);
  for (const [i, item] of progressItems.entries()) {
    const mark = item.firstElementChild;
    item.className = i < index ? "done" : i === index ? (failed ? "failed" : "active") : "";
    mark.textContent = i < index ? "✓" : i === index && failed ? "!" : "";
    mark.setAttribute("aria-label", i < index ? "Terminé" : i === index ? (failed ? "Interrompu" : "En cours") : "À venir");
  }
  showEvents(status.events || []);
  display("log", Boolean(status.events?.length));
  display("progress-actions", failed);
}
function messageTitle(id, title) {
  const heading = el(id).querySelector("h1");
  if (heading.textContent !== title) heading.textContent = title;
}
function httpUrl(value) {
  try {
    const url = new URL(value);
    return ["http:", "https:"].includes(url.protocol) && !url.username && !url.password ? url : null;
  } catch { return null; }
}
function complete(result) {
  pane("success", 3);
  const url = result.tunnelError ? null : httpUrl(result.url);
  const setup = el("open-setup");
  if (url) setup.href = url.href;
  else setup.removeAttribute("href");
  display("open-setup", Boolean(url));
  const rows = [];
  if (url) rows.push([result.ssh ? "Adresse via le tunnel" : "Adresse de SoundTime", url.origin]);
  if (result.sshHost) rows.push(["Hôte SSH", result.sshHost]);
  const lan = httpUrl(result.lanUrl);
  if (lan) rows.push(["Adresse sur le réseau", lan.origin]);
  rows.push(["Configuration", result.directory]);
  if (result.musicDirectory) rows.push(["Musique existante", result.musicDirectory]);
  display("sync-hint", Boolean(result.musicDirectory));
  table("result-details", rows);
  display("lan-unlock", Boolean(result.lanPending));
  message("lan-error", result.lanError || "");
  display("lan-error", Boolean(result.lanError));
  display("remote-next", Boolean(result.ssh));
  display("public-guide", Boolean(result.ssh));
  if (result.ssh) {
    const privatePort = Number(plan?.port);
    const origin = Number.isInteger(privatePort) ? "127.0.0.1:" + privatePort : "le port web de SoundTime sur 127.0.0.1";
    message("proxy-guide", "Après la création du compte, faites pointer votre nom de domaine vers le serveur. Configurez votre proxy HTTPS sur cet hôte pour transmettre les requêtes à " + origin + ". Autorisez le port HTTPS dans le pare-feu du serveur et dans celui de l’hébergeur.");
  }
  if (result.ssh) {
    message("remote-instructions", result.tunnelError
      ? String(result.tunnelError) + " Utilisez la commande ci-dessous pour ouvrir la connexion depuis votre ordinateur."
      : "L’assistant garde le tunnel SSH ouvert pour cette session. Pour retrouver l’accès privé après sa fermeture, exécutez cette commande dans un terminal.");
    const fallback = httpUrl(result.setupManualUrl);
    display("manual-url", Boolean(result.tunnelError && fallback));
    if (fallback) el("manual-url").href = fallback.href;
    else el("manual-url").removeAttribute("href");
    message("tunnel-cmd", result.setupTunnel || "");
    display("tunnel-cmd", Boolean(result.setupTunnel));
  }
  const text = result.tunnelError ? "Les services sont prêts sur la machine distante. Ouvrez un tunnel pour terminer la configuration."
    : lan
    ? "L’accès réseau est activé. Ouvrez l’adresse indiquée sur un appareil connecté au même réseau."
    : url?.pathname === "/setup"
      ? "Ouvrez votre serveur musical pour créer votre compte administrateur et ajouter vos premiers morceaux."
      : "Votre bibliothèque est accessible. Ouvrez SoundTime pour retrouver votre musique.";
  el("success").querySelector(".lead").textContent = text;
}
function handleStatus(status) {
  clearTimeout(timer);
  if (status.state === "complete") complete(status.result);
  else if (status.state === "failed" || status.state === "running") {
    progress(status);
    if (status.state === "running") timer = setTimeout(poll, 1200);
  } else if (status.state === "idle") pane("choose", 0);
  else throw new Error("Le lanceur a renvoyé un état inconnu. Rouvrez l’assistant.");
}
async function poll() {
  try {
    handleStatus(await request("status"));
  } catch (error) {
    notice(error.message + " Une nouvelle vérification de l’état aura lieu automatiquement.");
    timer = setTimeout(poll, 3500);
  }
}
async function launch(action, body) {
  progress({state: "running", stage: action === "lan" ? "start" : "prepare",
    message: action === "lan" ? "Activation de l’accès réseau…" : "Envoi de votre configuration au lanceur…", events: []});
  try {
    handleStatus(await request(action, body));
  } catch (error) {
    // The command may have reached the launcher even if the browser missed its response.
    try {
      const actual = await request("status");
      handleStatus(actual);
      if (actual.state !== "running") notice(error.message);
    } catch {
      notice(error.message + " Vérification de l’état de l’installation en cours…");
      timer = setTimeout(poll, 3000);
    }
  }
}
el("install").addEventListener("click", () => {
  if (exporting()) return exportProject();
  return sshMode() ? launch("remote/install", remoteBody()) : launch("install", plan);
});
el("enable-lan").addEventListener("click", () => launch("lan", {}));

async function connect() {
  pane("connect", 0, false);
  display("reconnect", false);
  if (!token) {
    messageTitle("connect", "Ouvrez le lanceur SoundTime.");
    message("connect-message", "Lancez l’installateur sur votre ordinateur, puis ouvrez son lien personnel pour choisir la machine qui accueillera votre musique.");
    return;
  }
  message("connect-message", "Connexion au lanceur SoundTime…");
  try {
    const session = await request("session");
    hasSession = true;
    defaults = session.defaults;
    machine = session.platform;
    message("summary-machine", machine);
    let saved;
    try { saved = JSON.parse(stored.read("soundtime.plan")); } catch { /* Use fresh defaults. */ }
    restoreDestination(saved);
    fillFields({...defaults, ...saved?.values}, saved?.mode);
    plan = settings();
    handleStatus(session.status);
  } catch (error) {
    messageTitle("connect", "Retrouver le lanceur");
    message("connect-message", error.message);
    display("reconnect", true);
  }
}
el("reconnect").addEventListener("click", connect);
el("exit").addEventListener("click", async () => {
  el("exit").disabled = true;
  try {
    await request("close", {});
    clearTimeout(timer);
    stored.remove("soundtime.plan");
    stored.remove("soundtime.session");
    token = null;
    hasSession = false;
    plan = undefined;
    form.reset();
    el("s3-access").value = "";
    el("s3-secret").value = "";
    pane("closed", ["success", "exported"].includes(currentPane) ? 3 : 0, false);
  } catch (error) { notice(error.message); }
  finally { el("exit").disabled = false; }
});
const help = el("help-dialog");
el("help-open").addEventListener("click", () => help.showModal());
help.querySelector(".dialog-close").addEventListener("click", () => help.close());
document.addEventListener("DOMContentLoaded", connect);
