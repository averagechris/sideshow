(() => {
  "use strict";

  const LOGICAL_W = 1920;
  const LOGICAL_H = 1080;
  const script = document.getElementById("sideshow-review-script");
  const nonce = script && script.dataset ? script.dataset.nonce : "";
  const stage = document.getElementById("stage");
  const slides = Array.from(document.querySelectorAll(".slide"));
  if (!nonce || !stage || !slides.length) return;

  const state = {
    revision: 0,
    etag: null,
    annotations: [],
    selectedId: null,
    editing: null,
    draft: null,
    drag: null,
    message: "",
    busy: false,
  };

  const root = el("aside", { className: "sideshow-review", role: "complementary", ariaLabel: "Sideshow review annotations" });
  const toolbar = el("div", { className: "sideshow-review-toolbar" });
  const list = el("div", { className: "sideshow-review-list", role: "list" });
  const editor = el("form", { className: "sideshow-review-editor", ariaLabel: "Annotation editor" });
  const status = el("div", { className: "sideshow-review-status", role: "status", ariaLive: "polite" });
  const overlay = el("div", { className: "sideshow-review-overlay", ariaLabel: "Slide annotation layer" });
  const editorTarget = el("div", { className: "sideshow-review-editor-target" });
  overlay.tabIndex = 0;
  stage.appendChild(overlay);
  document.body.appendChild(root);
  document.body.classList.add("sideshow-review-active");

  let kindInput, actionInput, bodyInput, intentDetails;
  buildChrome();
  stopDeckKeys(root);
  restoreDraft();
  overlay.addEventListener("pointerdown", beginPlacement);
  overlay.addEventListener("keydown", keyboardPlacement);
  document.addEventListener("pointermove", continuePlacement);
  document.addEventListener("pointerup", finishPlacement);
  document.addEventListener("pointercancel", cancelPlacement);
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      if (state.drag) { state.drag = null; renderOverlay(); }
      persistDraft();
      setPanelCollapsed(true);
      return;
    }
    if (event.key.toLowerCase() === "r" && !event.metaKey && !event.ctrlKey && !event.altKey && !isTypingTarget(event.target)) {
      event.preventDefault();
      setPanelCollapsed(!root.classList.contains("is-collapsed"));
      return;
    }
    if ((state.draft || state.editing) && !root.contains(event.target) && ["ArrowRight", "ArrowDown", " ", "PageDown", "ArrowLeft", "ArrowUp", "PageUp", "Home", "End", "Enter"].includes(event.key)) {
      event.preventDefault();
      event.stopImmediatePropagation();
      setStatus("Save or cancel the current annotation before changing slides.", true);
    }
  }, true);
  window.addEventListener("beforeunload", persistDraft);
  window.addEventListener("hashchange", scheduleRender);
  window.addEventListener("resize", fitStage);
  if (window.ResizeObserver) new ResizeObserver(fitStage).observe(root);
  new MutationObserver(scheduleRender).observe(stage, { subtree: true, attributes: true, attributeFilter: ["class"] });
  fitStage();
  load();

  function buildChrome() {
    toolbar.append(
      button("←", "Previous slide", () => navigate("prev")),
      button("→", "Next slide", () => navigate("next")),
      panelToggle(),
      button("Reload", "Reload annotations", load),
    );

    kindInput = select("Kind", [["note", "Note"], ["issue", "Issue"], ["question", "Question"], ["praise", "Praise"]]);
    actionInput = select("Action", [["", "No action"], ["fix", "Fix"], ["explain", "Explain"], ["test", "Test"], ["follow_up", "Follow up"]]);
    bodyInput = el("textarea", { rows: 5, placeholder: "Leave feedback…", ariaLabel: "Annotation body" });
    bodyInput.addEventListener("keydown", (event) => {
      if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) editor.requestSubmit();
    });
    [kindInput, actionInput, bodyInput].forEach((control) => control.addEventListener("input", persistDraft));
    intentDetails = el("details", { className: "sideshow-review-intent" });
    intentDetails.append(
      el("summary", { textContent: "Add intent (optional)" }),
      el("div", { className: "sideshow-review-intent-fields" },
        labelWrap("Comment type", kindInput),
        labelWrap("Suggested response", actionInput),
      ),
    );
    const save = el("button", { type: "submit", textContent: "Save" });
    const cancel = button("Cancel", "Cancel editing", cancelDraft);
    editor.append(editorTarget, labelWrap("Comment", bodyInput), intentDetails, el("div", { className: "sideshow-review-editor-actions" }, save, cancel));
    editor.addEventListener("submit", saveDraft);
    root.append(toolbar, status, list, editor);
  }

  function activeSlide() { return document.querySelector(".slide.is-active") || slides[0]; }
  function slideKey(slide) { return { slide_id: slide.id || "", source_path: slide.dataset.src || "" }; }
  function matchesSlide(a, slide) { const k = slideKey(slide); return a.slide_id === k.slide_id && a.source_path === k.source_path; }
  function matchesSlideId(a, slide) { return !!slide && a.slide_id === (slide.id || ""); }
  function knownPair(a) { return slides.some((s) => matchesSlide(a, s)); }

  async function load() {
    try {
      setStatus("Loading annotations…");
      const res = await fetch("/__sideshow/review", { headers: { "X-Sideshow-Review": nonce } });
      if (!res.ok) throw new Error(`GET failed (${res.status})`);
      applySnapshot(await res.json(), res.headers.get("ETag"));
      setStatus("Click to pin a comment. Drag to mark an area.");
    } catch (err) { setStatus(`Could not load review data: ${err.message}`); }
  }

  async function mutate(payload) {
    if (state.busy) return false;
    if (!state.etag) {
      setStatus("Review revision is not loaded yet. Reload annotations and try again.", true);
      return false;
    }
    state.busy = true;
    try {
      const headers = { "Content-Type": "application/json", "X-Sideshow-Review": nonce };
      headers["If-Match"] = state.etag;
      const res = await fetch("/__sideshow/review", { method: "POST", headers, body: JSON.stringify(payload) });
      const snap = await res.json().catch(() => null);
      if (res.status === 409) {
        if (snap) applySnapshot(snap, res.headers.get("ETag"));
        setStatus("Conflict: annotations changed on the server. Your change was not saved; review the refreshed list and try again.", true);
        return false;
      }
      if (!res.ok) throw new Error(`POST failed (${res.status})`);
      applySnapshot(snap, res.headers.get("ETag"));
      setStatus("Saved.");
      return true;
    } catch (err) { setStatus(`Could not save: ${err.message}`, true); return false; }
    finally { state.busy = false; }
  }

  function applySnapshot(snap, etag) {
    if (!snap || snap.schema_version !== 2 || !Array.isArray(snap.annotations)) throw new Error("unexpected review schema");
    if (!Number.isSafeInteger(snap.revision) || snap.revision < 0) throw new Error("invalid review revision");
    if (etag !== `"${snap.revision}"`) throw new Error("review ETag does not match snapshot revision");
    if (snap.revision < state.revision) return;
    state.revision = snap.revision;
    state.etag = etag;
    state.annotations = snap.annotations;
    render();
  }

  function beginPlacement(event) {
    if (event.button !== 0 || event.target.closest(".sideshow-review-marker")) return;
    if (state.draft || state.editing) {
      setStatus("Save or cancel the current annotation before placing another.", true);
      return;
    }
    const p = toLogical(event);
    const slide = activeSlide();
    const hints = hintsFor(event);
    state.drag = {
      slide,
      start: p,
      current: p,
      startClient: { x: event.clientX, y: event.clientY },
      currentClient: { x: event.clientX, y: event.clientY },
      hints,
      pointerId: event.pointerId,
    };
    try { overlay.setPointerCapture(event.pointerId); } catch (_) {}
    renderOverlay();
    event.preventDefault();
  }
  function continuePlacement(event) {
    if (state.drag && state.drag.pointerId === event.pointerId) {
      state.drag.current = toLogical(event);
      state.drag.currentClient = { x: event.clientX, y: event.clientY };
      renderOverlay();
    }
  }
  function finishPlacement(event) {
    if (!state.drag || state.drag.pointerId !== event.pointerId) return;
    const d = state.drag;
    d.current = toLogical(event);
    d.currentClient = { x: event.clientX, y: event.clientY };
    state.drag = null;
    newDraft({ slide: d.slide, target: placementTarget(d) });
  }
  function cancelPlacement(event) { if (state.drag && state.drag.pointerId === event.pointerId) cancelDraft(); }

  function newDraft({ slide, target }) {
    const k = slideKey(slide);
    state.selectedId = null;
    state.draft = { slide_id: k.slide_id, source_path: k.source_path, target, body: "", kind: "note", action: null };
    kindInput.value = "note"; actionInput.value = ""; bodyInput.value = "";
    intentDetails.open = false;
    setPanelCollapsed(false); persistDraft(); render();
    bodyInput.focus();
  }
  function editAnnotation(a) {
    state.selectedId = a.id; state.draft = null;
    state.editing = { id: a.id, slide_id: a.slide_id, source_path: a.source_path, target: a.target };
    kindInput.value = a.kind || "note"; actionInput.value = a.action || ""; bodyInput.value = a.body || "";
    intentDetails.open = a.kind !== "note" || !!a.action;
    setPanelCollapsed(false); persistDraft(); render(); bodyInput.focus();
  }
  function cancelDraft() { state.draft = null; state.drag = null; state.editing = null; state.selectedId = null; bodyInput.value = ""; clearPersistedDraft(); render(); }
  async function saveDraft(event) {
    event.preventDefault();
    const body = bodyInput.value.trim();
    if (!body) return setStatus("Write a comment before saving.", true);
    const kind = kindInput.value; const action = actionInput.value || null;
    if (state.editing) {
      const existing = state.annotations.find((a) => a.id === state.editing.id);
      const payload = existing
        ? { operation: "edit", revision: state.revision, id: state.editing.id, body, kind, action }
        : { operation: "create", revision: state.revision, annotation: { slide_id: state.editing.slide_id, source_path: state.editing.source_path, target: state.editing.target, body, kind, action } };
      if (await mutate(payload)) cancelDraft();
      return;
    }
    if (!state.draft) return setStatus("Click or drag on the slide before saving.", true);
    if (await mutate({ operation: "create", revision: state.revision, annotation: { ...state.draft, body, kind, action } })) {
      cancelDraft();
    }
  }

  function render() {
    renderList();
    renderOverlay();
    const target = state.draft || state.editing;
    editor.classList.toggle("is-open", !!target);
    editorTarget.textContent = target ? `${state.editing && !state.annotations.some((a) => a.id === state.editing.id) ? "Recreate deleted comment on" : "Comment on"} ${slideLabel(target)}` : "";
  }
  function scheduleRender() { window.requestAnimationFrame(render); }
  function renderList() {
    list.replaceChildren();
    const slide = activeSlide();
    const current = state.annotations.filter((a) => {
      const freshness = annotationFreshness(a);
      return freshness !== "orphaned" && (matchesSlide(a, slide) || (freshness === "stale" && matchesSlideId(a, slide)));
    });
    const orphaned = state.annotations.filter((a) => annotationFreshness(a) === "orphaned");
    list.append(el("h2", { textContent: "Active slide" }));
    if (!current.length) list.append(el("p", { className: "sideshow-review-empty", textContent: "No annotations on this slide." }));
    current.forEach((a) => list.append(item(a, false)));
    if (orphaned.length) {
      list.append(el("h2", { textContent: "Orphaned annotations" }));
      orphaned.forEach((a) => list.append(item(a, true)));
    }
  }
  function item(a, orphan) {
    const freshness = annotationFreshness(a);
    const disposition = annotationDisposition(a);
    const node = el("article", { className: `sideshow-review-item is-${freshness}${a.id === state.selectedId ? " is-selected" : ""}${orphan ? " is-orphan" : ""}`, role: "listitem" });
    const metaText = [a.kind && a.kind !== "note" ? a.kind : "", a.action || "", freshness, a.state === "resolved" ? "resolved" : "", disposition ? `disposition: ${disposition.replace(/_/g, " ")}` : ""].filter(Boolean).join(" · ");
    const meta = el("div", { className: "sideshow-review-meta", textContent: metaText });
    meta.hidden = !metaText;
    const body = el("p", { className: "sideshow-review-body" }); body.textContent = a.body || "(empty)";
    const dispositionNote = disposition && typeof a.disposition_note === "string" && a.disposition_note.trim()
      ? el("p", { className: "sideshow-review-disposition-note", textContent: `Disposition note: ${a.disposition_note}` })
      : null;
    const detail = el("details", { className: "sideshow-review-detail" });
    detail.append(el("summary", { textContent: orphan ? `${a.source_path} (orphaned)` : "Target details" }), el("pre", { textContent: targetDescription(a) }));
    const actions = el("div", { className: "sideshow-review-actions" });
    actions.append(button("Edit", "Edit annotation", () => editAnnotation(a)), button(a.state === "resolved" ? "Reopen" : "Resolve", "Toggle resolution", () => mutate({ operation: "set_state", revision: state.revision, id: a.id, state: a.state === "resolved" ? "todo" : "resolved" })), button("Delete", "Delete annotation", () => mutate({ operation: "delete", revision: state.revision, id: a.id })));
    node.append(meta, body);
    if (dispositionNote) node.append(dispositionNote);
    node.append(detail, actions); return node;
  }
  function renderOverlay() {
    const focusedId = document.activeElement && document.activeElement.dataset ? document.activeElement.dataset.annotationId : null;
    overlay.replaceChildren();
    const slide = activeSlide();
    state.annotations.filter((a) => matchesSlide(a, slide)).forEach((a, i) => overlay.append(marker(a, i + 1)));
    if (state.draft && matchesSlide(state.draft, slide)) overlay.append(marker({ ...state.draft, id: "draft", state: "todo" }));
    if (state.drag) overlay.append(marker({ target: placementTarget(state.drag), kind: "note", state: "todo", id: "draft-placement" }));
    if (focusedId) Array.from(overlay.querySelectorAll("[data-annotation-id]")).find((node) => node.dataset.annotationId === focusedId)?.focus();
  }
  function marker(a, number) {
    const t = a.target || {}; const region = t.type === "region";
    const m = el(region ? "div" : "button", { className: `sideshow-review-marker is-${a.kind || "note"}${a.state === "resolved" ? " is-resolved" : ""}`, ariaLabel: `Annotation: ${a.kind || "note"}` });
    if (!region) m.type = "button";
    if (t.type === "region") Object.assign(m.style, { left: `${t.x / LOGICAL_W * 100}%`, top: `${t.y / LOGICAL_H * 100}%`, width: `${t.width / LOGICAL_W * 100}%`, height: `${t.height / LOGICAL_H * 100}%` });
    else Object.assign(m.style, { left: `${t.x / LOGICAL_W * 100}%`, top: `${t.y / LOGICAL_H * 100}%` });
    m.dataset.shape = t.type || "point";
    const editable = a.id && a.id !== "draft" && a.id !== "draft-placement";
    const handle = region && editable ? el("button", { className: "sideshow-review-region-handle", type: "button", textContent: String(number || ""), ariaLabel: `Edit ${a.kind || "note"} region annotation` }) : m;
    if (a.id) handle.dataset.annotationId = a.id;
    if (number && !region) m.textContent = String(number);
    if (handle !== m) m.append(handle);
    if (editable) {
      handle.addEventListener("click", (e) => { e.stopPropagation(); editAnnotation(a); });
      handle.addEventListener("keydown", (e) => e.stopPropagation());
    }
    return m;
  }

  function placementTarget(d) {
    const dx = d.currentClient.x - d.startClient.x;
    const dy = d.currentClient.y - d.startClient.y;
    if (Math.hypot(dx, dy) < 6) return { type: "point", x: d.start.x, y: d.start.y, ...d.hints };
    const rawWidth = Math.abs(d.current.x - d.start.x);
    const rawHeight = Math.abs(d.current.y - d.start.y);
    return {
      type: "region",
      x: Math.min(d.start.x, d.current.x),
      y: Math.min(d.start.y, d.current.y),
      width: Math.max(1, rawWidth),
      height: Math.max(1, rawHeight),
      ...d.hints,
    };
  }
  function toLogical(event) { const r = stage.getBoundingClientRect(); return { x: clamp((event.clientX - r.left) / r.width * LOGICAL_W, 0, LOGICAL_W), y: clamp((event.clientY - r.top) / r.height * LOGICAL_H, 0, LOGICAL_H) }; }
  function hintsFor(event) {
    const old = overlay.style.pointerEvents; overlay.style.pointerEvents = "none";
    const target = document.elementFromPoint(event.clientX, event.clientY);
    overlay.style.pointerEvents = old;
    if (!target || target === stage || target.classList.contains("slide")) return {};
    const out = {}; const id = target.id ? `#${target.id}` : ""; const cls = target.className && typeof target.className === "string" ? `.${target.className.trim().split(/\s+/).slice(0, 2).join(".")}` : "";
    if (id || cls) out.selector_hint = `${target.tagName.toLowerCase()}${id}${cls}`;
    const text = (target.textContent || "").replace(/\s+/g, " ").trim(); if (text) out.text_hint = text.slice(0, 120);
    const plan = target.closest("[data-plan-kind][data-plan-id]");
    if (plan) {
      out.plan_kind = (plan.dataset.planKind || "").slice(0, 256);
      out.plan_id = (plan.dataset.planId || "").slice(0, 256);
      if (!out.plan_kind) delete out.plan_kind;
      if (!out.plan_id) delete out.plan_id;
    }
    return out;
  }
  function panelToggle() {
    const control = button("Panel", "Collapse or expand review panel (R)", () => setPanelCollapsed(!root.classList.contains("is-collapsed")));
    control.classList.add("sideshow-review-panel-toggle");
    control.setAttribute("aria-keyshortcuts", "R");
    control.setAttribute("aria-expanded", "true");
    return control;
  }
  function setPanelCollapsed(collapsed) {
    root.classList.toggle("is-collapsed", collapsed);
    root.querySelector(".sideshow-review-panel-toggle")?.setAttribute("aria-expanded", String(!collapsed));
    window.requestAnimationFrame(fitStage);
  }
  function fitStage() {
    const margin = 16;
    let left = margin;
    let right = window.innerWidth - margin;
    let top = margin;
    let bottom = window.innerHeight - margin;
    if (!root.classList.contains("is-collapsed")) {
      const panel = root.getBoundingClientRect();
      const bottomPanel = window.matchMedia("(max-width: 760px)").matches;
      if (bottomPanel) bottom = Math.max(top, panel.top - margin);
      else right = Math.max(left, panel.left - margin);
    }
    const scale = Math.max(0.05, Math.min((right - left) / LOGICAL_W, (bottom - top) / LOGICAL_H));
    stage.style.left = `${left + (right - left) / 2}px`;
    stage.style.top = `${top + (bottom - top) / 2}px`;
    document.documentElement.style.setProperty("--sideshow-scale", String(scale));
  }
  function navigate(direction) {
    if (state.draft || state.editing) return setStatus("Save or cancel the current annotation before changing slides.", true);
    if (window.sideshow) window.sideshow[direction]();
  }
  function keyboardPlacement(event) {
    if (!["Enter", " "].includes(event.key) || event.target.closest(".sideshow-review-marker")) return;
    event.preventDefault(); event.stopPropagation();
    const slide = activeSlide();
    newDraft({ slide, target: { type: "point", x: LOGICAL_W / 2, y: LOGICAL_H / 2 } });
  }
  function targetDescription(a) {
    const t = a.target || {};
    const shape = t.type === "region" ? `region ${round(t.x)},${round(t.y)} ${round(t.width)}×${round(t.height)}` : `point ${round(t.x)},${round(t.y)}`;
    return [`slide: ${a.slide_id}`, `source: ${a.source_path}`, `target: ${shape}`, t.selector_hint ? `selector: ${t.selector_hint}` : "", t.text_hint ? `text: ${t.text_hint}` : "", t.plan_kind ? `untrusted plan kind: ${t.plan_kind}` : "", t.plan_id ? `untrusted plan id: ${t.plan_id}` : ""].filter(Boolean).join("\n");
  }
  function annotationFreshness(a) {
    const server = ["current", "stale", "orphaned"].includes(a.freshness) ? a.freshness : null;
    return server || (knownPair(a) ? "current" : "orphaned");
  }
  function annotationDisposition(a) { return ["addressed", "wont_fix", "deferred"].includes(a.disposition) ? a.disposition : null; }
  function slideLabel(target) {
    const index = slides.findIndex((slide) => matchesSlide(target, slide));
    return index >= 0 ? `slide ${index + 1}` : target.source_path;
  }
  function isTypingTarget(target) { return target instanceof Element && (target.matches("input, textarea, select") || target.isContentEditable); }
  function persistDraft() {
    if (!state.draft && !state.editing) return clearPersistedDraft();
    try { sessionStorage.setItem(`sideshow-review-draft:${nonce}`, JSON.stringify({ draft: state.draft, editing: state.editing, kind: kindInput.value, action: actionInput.value, body: bodyInput.value })); } catch (_) {}
  }
  function restoreDraft() {
    try {
      const saved = JSON.parse(sessionStorage.getItem(`sideshow-review-draft:${nonce}`) || "null");
      if (!saved || (!saved.draft && !saved.editing)) return;
      state.draft = saved.draft || null; state.editing = saved.editing || null; state.selectedId = state.editing && state.editing.id;
      kindInput.value = saved.kind || "note"; actionInput.value = saved.action || ""; bodyInput.value = saved.body || "";
    } catch (_) { clearPersistedDraft(); }
  }
  function clearPersistedDraft() { try { sessionStorage.removeItem(`sideshow-review-draft:${nonce}`); } catch (_) {} }
  function setStatus(text, warn) { state.message = text; status.textContent = text; status.classList.toggle("is-warning", !!warn); }
  function stopDeckKeys(node) { node.addEventListener("keydown", (e) => e.stopPropagation(), true); }
  function button(text, label, fn) { const b = el("button", { type: "button", textContent: text, ariaLabel: label }); b.addEventListener("click", fn); return b; }
  function select(label, options) { const s = el("select", { ariaLabel: label }); options.forEach(([value, text]) => s.append(el("option", { value, textContent: text }))); return s; }
  function labelWrap(text, control) { return el("label", {}, el("span", { textContent: text }), control); }
  function el(tag, props, ...kids) { const n = document.createElement(tag); Object.entries(props || {}).forEach(([k, v]) => { if (k === "className") n.className = v; else if (k === "textContent") n.textContent = v; else if (k === "ariaLabel") n.setAttribute("aria-label", v); else n[k] = v; }); kids.forEach((k) => n.append(k)); return n; }
  function clamp(n, min, max) { return Math.max(min, Math.min(max, n)); }
  function round(n) { return Number.isFinite(Number(n)) ? Math.round(Number(n)) : "?"; }
})();
