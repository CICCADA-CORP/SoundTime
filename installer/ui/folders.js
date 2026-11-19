"use strict";

(function () {
  const dialog = el("folder-dialog");
  const select = el("folder-select");
  const list = el("folder-list");
  const roots = el("folder-roots");
  const name = el("folder-name");
  let target;
  let current;
  let requestId = 0;

  function chooseFolder(item) {
    const row = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    const icon = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    icon.classList.add("ico");
    icon.setAttribute("aria-hidden", "true");
    const use = document.createElementNS(icon.namespaceURI, "use");
    use.setAttribute("href", "#folder");
    icon.append(use);
    button.append(icon, document.createTextNode(item.name));
    button.addEventListener("click", () => browse(item.path));
    row.append(button);
    return row;
  }

  function paint(data) {
    current = data;
    message("folder-location", data.path);
    el("folder-up").disabled = !data.parent;
    select.disabled = false;
    roots.replaceChildren(new Option("Emplacements…", ""), ...data.roots.map(root => new Option(root.name, root.path)));
    roots.hidden = false;
    roots.disabled = false;
    list.replaceChildren(...data.folders.map(chooseFolder));
    list.scrollTop = 0;
    message("folder-status", data.folders.length ? "" : "Ce dossier ne contient aucun sous-dossier visible.");
  }

  async function browse(path, initial = false) {
    const id = ++requestId;
    const valid = () => dialog.open && id === requestId;
    select.disabled = true;
    el("folder-up").disabled = true;
    roots.disabled = true;
    list.inert = true;
    message("folder-status", "Lecture des dossiers…");
    try {
      let data;
      try {
        data = await request("folders", {path});
      } catch (error) {
        if (!initial || !path || !valid()) throw error;
        // The default install directory usually does not exist yet. Open its parent instead.
        const split = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
        if (target.id === "directory" && split >= 0 && split < path.length - 1) {
          const parent = path.slice(0, split + 1);
          data = await request("folders", {path: parent}).catch(() => null);
          if (data && valid()) name.value = path.slice(split + 1);
        }
        if (!data) data = await request("folders", {path: ""});
      }
      if (valid()) {
        const moving = Boolean(current);
        paint(data);
        if (moving) el("folder-location").focus({preventScroll: true});
      }
    } catch (error) {
      if (!valid()) return;
      if (current) paint(current);
      message("folder-status", error.message);
    } finally {
      if (valid()) list.inert = false;
    }
  }

  function openPicker(input) {
    target = input;
    current = null;
    name.value = "";
    name.setCustomValidity("");
    message("folder-title", input.id === "directory" ? "Dossier de configuration" : "Dossier de musique");
    message("folder-location", input.value || "Dossier personnel");
    display("folder-new", input.id === "directory");
    list.replaceChildren();
    roots.hidden = true;
    dialog.showModal();
    void browse(input.value.trim(), true);
  }

  document.querySelectorAll("[data-browse]").forEach(button => {
    button.addEventListener("click", () => openPicker(el(button.dataset.browse)));
  });
  roots.addEventListener("change", () => { if (roots.value) void browse(roots.value); });
  el("folder-up").addEventListener("click", () => { if (current?.parent) void browse(current.parent); });
  el("folder-close").addEventListener("click", () => dialog.close());
  el("folder-cancel").addEventListener("click", () => dialog.close());
  name.addEventListener("input", () => name.setCustomValidity(""));
  select.addEventListener("click", () => {
    if (!current) return;
    const leaf = target.id === "directory" ? name.value.trim() : "";
    if (leaf === "." || leaf === ".." || /[\\/]/.test(leaf) || (machine === "Windows" && /[<>:"|?*]/.test(leaf))) {
      name.setCustomValidity("Entrez uniquement un nom de sous-dossier.");
      name.reportValidity();
      return;
    }
    const separator = machine === "Windows" ? "\\" : "/";
    const base = current.path;
    target.value = leaf ? base + (base.endsWith(separator) ? "" : separator) + leaf : base;
    target.dispatchEvent(new Event("change", {bubbles: true}));
    dialog.close();
  });
})();
