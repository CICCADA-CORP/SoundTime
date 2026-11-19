"use strict";

const targetNotes = {
  nas: "Utilisez un NAS AMD64 ou ARM64 équipé de Synology Container Manager, de QNAP Container Station ou d’un autre gestionnaire Docker compatible.",
  server: "Utilisez un serveur Linux AMD64 ou ARM64 avec Docker et Compose. Gardez le port web privé pendant la configuration.",
  vps: "Prévoyez un VPS Linux avec un disque persistant. Le port web peut rester privé derrière un tunnel ou un proxy HTTPS.",
  vm: "Choisissez une VM Linux AMD64 ou ARM64 avec un disque persistant. Pour SSH, son adresse doit être accessible depuis cet ordinateur.",
  cloud: "Utilisez une instance Linux avec un disque persistant. Autorisez SSH depuis votre adresse dans le pare-feu du fournisseur."
};
const remote = () => form.elements.scope.value === "remote";
const exporting = () => remote() && el("method").value === "export";
const sshMode = () => remote() && el("method").value === "ssh";
const localCustom = () => !remote() && form.elements.mode.value === "custom";
const targetLabel = () => el("target").selectedOptions[0].textContent;
const targetName = () => remote() ? targetLabel() : machine;
const remoteBody = () => ({
  target: el("target").value,
  settings: plan,
  ...(sshMode() && {ssh: {
    host: el("ssh-host").value.trim(), user: el("ssh-user").value.trim(), port: el("ssh-port").valueAsNumber
  }})
});

function remotePath() {
  if (sshMode()) return "~/SoundTime Server";
  if (el("target").value === "nas") {
    if (el("nas-system").value === "synology") return "/volume1/docker/soundtime";
    if (el("nas-system").value === "qnap") return "/share/Container/soundtime";
  }
  return "/opt/soundtime";
}
function storageSettings() {
  const pick = el("storage").value;
  if (pick === "local") return {kind: "local"};
  return {
    kind: "s3", region: el("s3-region").value.trim(), bucket: el("s3-bucket").value.trim(),
    endpoint: pick === "compatible" ? el("s3-endpoint").value.trim() : "",
    prefix: el("s3-prefix").value.trim(), accessKey: el("s3-access").value, secretKey: el("s3-secret").value
  };
}
function restoreDestination(saved) {
  form.elements.scope.value = saved?.scope === "remote" ? "remote" : "local";
  el("target").value = Object.hasOwn(targetNotes, saved?.target) ? saved.target : "nas";
  el("method").value = saved?.method === "ssh" ? "ssh" : "export";
  el("nas-system").value = ["synology","qnap","other"].includes(saved?.nasSystem) ? saved.nasSystem : "synology";
  el("ssh-host").value = saved?.ssh?.host || "";
  el("ssh-user").value = saved?.ssh?.user || "";
  el("ssh-port").value = saved?.ssh?.port || 22;
  const storage = saved?.values?.storage;
  el("storage").value = storage?.kind === "s3" ? (saved?.provider === "aws" ? "aws" : "compatible") : "local";
  el("s3-region").value = storage?.region || "us-east-1";
  el("s3-bucket").value = storage?.bucket || "";
  el("s3-prefix").value = storage?.prefix || "";
}
function savePlan() {
  // Whitelist storage metadata. Endpoints may contain user input, including credentials.
  const {storage, ...values} = plan;
  values.storage = storage?.kind === "s3"
    ? {kind: "s3", region: storage.region, bucket: storage.bucket, prefix: storage.prefix}
    : {kind: "local"};
  const saved = {
    values, scope: form.elements.scope.value, mode: form.elements.mode.value, provider: el("storage").value,
    target: el("target").value, method: el("method").value, nasSystem: el("nas-system").value
  };
  if (sshMode()) saved.ssh = remoteBody().ssh;
  stored.write("soundtime.plan", JSON.stringify(saved));
}

let previousPanel;
let previousScope;
let previousSuggestedPath;
let localPaths;
function destinationFields(event) {
  const scope = form.elements.scope.value;
  const isRemote = remote();
  const isSSH = sshMode();
  const isExport = exporting();
  const target = el("target").value;
  const nasExport = isExport && target === "nas";
  const panel = el("settings-panel");
  if (previousScope !== undefined && previousScope !== scope) {
    if (isRemote) {
      localPaths = {directory: el("directory").value, music: el("music-directory").value, access: el("access").value};
      el("directory").value = remotePath();
      el("music-directory").value = "";
      el("access").value = nasExport ? "lan" : "local";
    } else {
      el("directory").value = localPaths?.directory || defaults.directory;
      el("music-directory").value = localPaths?.music || "";
      el("access").value = localPaths?.access || "local";
    }
  } else if (isRemote && ["method", "target", "nas-system"].includes(event?.target?.id)) {
    if (el("directory").value === previousSuggestedPath) el("directory").value = remotePath();
    if (event.target.id !== "nas-system") el("access").value = nasExport ? "lan" : "local";
  }
  previousScope = scope;
  previousSuggestedPath = isRemote ? remotePath() : undefined;
  const showSettings = isRemote || localCustom();
  const panelKind = isRemote ? "remote" : form.elements.mode.value;
  if (panelKind !== previousPanel) panel.open = localCustom();
  previousPanel = panelKind;
  panel.hidden = !showSettings;
  settingsFields.disabled = !showSettings;
  el("local-modes").disabled = isRemote;
  display("local-modes", !isRemote);
  display("remote-options", isRemote);
  el("remote-options").disabled = !isRemote;
  el("ssh-fields").disabled = !isSSH;
  display("ssh-fields", isSSH);
  display("nas-field", nasExport);
  el("nas-system").disabled = !nasExport;
  message("nas-path-hint", "Le dossier proposé est " + el("directory").value + ". Si votre partage utilise un autre chemin, modifiez le dossier de configuration ci-dessous.");
  for (const button of form.querySelectorAll("[data-browse]")) button.hidden = isRemote;
  message("target-hint", targetNotes[target]);
  message("method-hint", isSSH
    ? "L’assistant se connecte à la machine Linux, vérifie Python 3.9+, Docker et Compose 2.24.4+, puis installe SoundTime avec votre accord."
    : "Téléchargez un projet prêt à importer sur la machine choisie. Ce téléchargement fonctionne même si Docker est absent de cet ordinateur.");
  message("directory-hint", isRemote
    ? (isExport ? "Emplacement prévu sur la machine cible pour les fichiers du projet." : "Choisissez un dossier accessible en écriture à votre utilisateur SSH.")
    : "Choisissez un dossier vide ou un nouveau dossier pour les fichiers de configuration.");
  message("music-hint", isRemote ? "Entrez le chemin Linux complet sur la machine distante. SoundTime y lira les morceaux en lecture seule."
    : "SoundTime y lira vos morceaux. Laissez ce champ vide pour ajouter votre musique après l’installation.");
  const localOption = el("access").options[0];
  localOption.textContent = isRemote ? "Accès privé par tunnel ou proxy" : "Sur cet appareil uniquement";
  el("access").options[1].textContent = isRemote ? "Sur le réseau privé de la machine" : "Sur les appareils de mon réseau";
  const lan = el("access").value === "lan";
  message("remote-access-hint", lan
    ? (nasExport ? "La première ouverture sur le réseau du NAS est protégée par le mot de passe temporaire inclus avec le projet."
      : "Réservez cet accès aux réseaux de confiance. Pour un serveur sur Internet, préférez l’accès privé avec un proxy HTTPS.")
    : "Le port web reste sur la boucle locale du serveur. Pour publier votre instance, configurez un proxy HTTPS sur l’hôte après la création du compte administrateur.");
  display("remote-access-hint", isRemote);
  display("lan-hint", lan && !isExport);
  const objectStorage = el("storage").value !== "local";
  const compatible = el("storage").value === "compatible";
  display("s3-fields", objectStorage);
  el("s3-fields").disabled = !objectStorage;
  display("endpoint-field", compatible);
  el("s3-endpoint").disabled = !compatible;
  el("s3-endpoint").required = compatible;
  message("region-hint", compatible
    ? "Indiquez la région fournie par le service. Pour Cloudflare R2, utilisez auto."
    : "Indiquez la région de votre bucket AWS, par exemple eu-west-3.");
  el("music-directory").disabled = objectStorage;
  display("music-source", !objectStorage);
  message("summary-machine", targetName());
  const activeLan = showSettings ? lan : defaults.access === "lan";
  message("summary-access", activeLan ? "Réseau privé" : (isRemote ? "Accès privé" : "Cet appareil"));
  message("continue-label", isExport ? "Relire le projet" : (isSSH ? "Vérifier la connexion" : "Vérifier ma machine"));
  message("confirm-hint", isExport ? "Vous pourrez relire les réglages avant de télécharger." : "L’installation démarrera après votre confirmation.");
  steps[1].lastElementChild.textContent = isExport ? "Résumé" : "Vérification";
  steps[2].lastElementChild.textContent = isExport ? "Export" : "Installation";
}

function confirmExport() {
  messageTitle("check-view", "Relisez votre projet.");
  message("check-message", "Le projet contient la configuration à transférer. L’installation démarrera sur la machine cible lorsque vous y lancerez le projet.");
  el("check-message").classList.remove("busy");
  el("checks").replaceChildren();
  display("docker-guide", false);
  display("recheck", false);
  display("install", true);
  el("install").disabled = false;
  message("start-label", "Télécharger le projet");
  review();
  display("review", true);
  el("review").open = true;
  el("change").disabled = false;
  display("exit", true);
}
async function exportProject() {
  el("install").disabled = true;
  el("change").disabled = true;
  display("exit", false);
  notice();
  try {
    const archive = await request("export", remoteBody(), true);
    const objectURL = URL.createObjectURL(archive);
    const link = document.createElement("a");
    link.href = objectURL;
    link.download = "SoundTime-" + el("target").value + ".zip";
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(objectURL), 60000);
    const nas = el("target").value === "nas";
    const nasManager = el("nas-system").value;
    const startGuide = {
      synology: "Dans Container Manager, ouvrez Projet → Créer. Sélectionnez le dossier extrait et son fichier compose.yaml, puis démarrez le projet.",
      qnap: "Dans Container Station, ouvrez Applications → Créer, collez le contenu de qnap-compose.yaml et démarrez l’application. Le dossier qnap/ doit se trouver à l’emplacement extrait sur le NAS.",
      other: "Dans votre gestionnaire Docker, importez compose.yaml depuis le dossier extrait. Suivez la procédure Portainer du guide si vous utilisez Portainer."
    };
    message("export-transfer", "Extrayez tous les fichiers sur la machine cible, dans le dossier " + plan.directory + ".");
    message("export-start", nas
      ? startGuide[nasManager]
      : "Depuis le dossier extrait sur le serveur, lancez la commande docker compose up -d. Le guide inclus détaille les prérequis et les vérifications.");
    message("export-admin", nas && plan.access === "lan"
      ? "Sur votre réseau privé, ouvrez http://ADRESSE-DU-NAS:" + plan.port + "/setup. Utilisez le compte temporaire du fichier LISEZ-MOI.txt, puis suivez ses indications pour retirer la protection initiale après création de votre administrateur."
      : "Ouvrez le tunnel SSH indiqué dans LISEZ-MOI.txt, puis suivez le lien de première connexion du guide.");
    pane("exported", 3);
  } catch (error) { notice(error.message); }
  finally {
    el("install").disabled = false;
    el("change").disabled = false;
    display("exit", true);
  }
}
el("export-back").addEventListener("click", () => pane("choose", 0));
