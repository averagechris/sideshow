(async () => {
  "use strict";

  const waitFor = async (check, label, timeout = 5000) => {
    const deadline = Date.now() + timeout;
    while (Date.now() < deadline) {
      const value = check();
      if (value) return value;
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    throw new Error(`timed out waiting for ${label}`);
  };
  const assert = (condition, message) => {
    if (!condition) throw new Error(message);
  };
  const script = document.querySelector("#sideshow-review-script");
  const overlay = document.querySelector(".sideshow-review-overlay");
  const form = document.querySelector(".sideshow-review-editor");
  const body = form && form.querySelector("textarea");
  const panel = document.querySelector(".sideshow-review");
  const nonce = script && script.dataset.nonce;
  assert(nonce && overlay && panel && form && body, "review mode is not initialized");
  assert(!form.classList.contains("is-open"), "refusing to run with an unsaved review draft");

  await waitFor(() => {
    const stageRect = document.querySelector("#stage").getBoundingClientRect();
    const panelRect = panel.getBoundingClientRect();
    return window.innerWidth <= 760
      ? stageRect.bottom <= panelRect.top + 1
      : stageRect.right <= panelRect.left + 1;
  }, "stage fit around review panel");
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  await waitFor(() => panel.classList.contains("is-collapsed"), "Escape panel close");
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "r", bubbles: true }));
  await waitFor(() => !panel.classList.contains("is-collapsed"), "R panel open");

  const headers = { "X-Sideshow-Review": nonce };
  const snapshot = () => fetch("/__sideshow/review", { headers }).then((response) => response.json());
  const mutate = (payload) => fetch("/__sideshow/review", {
    method: "POST",
    headers: { ...headers, "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  const enterBody = (value) => {
    body.value = value;
    body.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const articleWithBody = (value) => Array.from(document.querySelectorAll(".sideshow-review-item"))
    .find((article) => article.querySelector(".sideshow-review-body")?.textContent === value);
  const save = () => form.requestSubmit();
  const unique = `review-smoke-${Date.now()}`;
  const pointBody = `${unique}-point`;

  const rect = document.querySelector("#stage").getBoundingClientRect();
  overlay.dispatchEvent(new PointerEvent("pointerdown", {
    bubbles: true,
    button: 0,
    pointerId: 41,
    clientX: rect.left + rect.width * 0.3,
    clientY: rect.top + rect.height * 0.3,
  }));
  document.dispatchEvent(new PointerEvent("pointerup", {
    bubbles: true,
    pointerId: 41,
    clientX: rect.left + rect.width * 0.3,
    clientY: rect.top + rect.height * 0.3,
  }));
  await waitFor(() => form.classList.contains("is-open"), "point editor");
  enterBody(pointBody);
  body.dispatchEvent(new KeyboardEvent("keydown", { key: "r", bubbles: true }));
  assert(!panel.classList.contains("is-collapsed"), "R toggled the panel while typing");
  body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  await waitFor(() => panel.classList.contains("is-collapsed"), "Escape close with draft");
  assert(body.value === pointBody, "Escape discarded the draft");
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "r", bubbles: true }));
  await waitFor(() => !panel.classList.contains("is-collapsed"), "R reopen with draft");
  save();
  await waitFor(() => articleWithBody(pointBody), "point annotation create");
  assert((await snapshot()).annotations.find((item) => item.body === pointBody)?.target.type === "point", "click was not inferred as a point");

  articleWithBody(pointBody).querySelector('[aria-label="Edit annotation"]').click();
  const editedBody = `${unique}-edited`;
  enterBody(editedBody);
  save();
  await waitFor(() => articleWithBody(editedBody), "annotation edit");

  articleWithBody(editedBody).querySelector('[aria-label="Toggle resolution"]').click();
  await waitFor(
    () => articleWithBody(editedBody)?.querySelector(".sideshow-review-meta")?.textContent.includes("resolved"),
    "annotation resolution",
  );

  const slide = document.querySelector(".slide.is-active");
  const originalSource = slide.dataset.src;
  slide.dataset.src = `${originalSource}.removed`;
  window.dispatchEvent(new HashChangeEvent("hashchange"));
  await waitFor(() => Array.from(document.querySelectorAll(".sideshow-review h2")).some((heading) => heading.textContent === "Orphaned annotations"), "orphan display");
  assert(document.querySelector(".sideshow-review-detail summary").textContent.includes("orphaned"), "orphan does not expose source identity");
  slide.dataset.src = originalSource;
  window.dispatchEvent(new HashChangeEvent("hashchange"));
  await waitFor(() => articleWithBody(editedBody), "orphan recovery");

  articleWithBody(editedBody).querySelector('[aria-label="Edit annotation"]').click();
  const recoveredBody = `${unique}-recovered`;
  enterBody(recoveredBody);
  const beforeDelete = await snapshot();
  const annotation = beforeDelete.annotations.find((item) => item.body === editedBody);
  assert(annotation, "edited annotation missing from API");
  const deleted = await mutate({ operation: "delete", revision: beforeDelete.revision, id: annotation.id });
  assert(deleted.status === 200, "simulated concurrent delete failed");
  save();
  await waitFor(() => document.querySelector(".sideshow-review-status").textContent.startsWith("Conflict:"), "conflict response");
  assert(form.classList.contains("is-open") && body.value === recoveredBody, "conflict discarded the local edit");
  assert(document.querySelector(".sideshow-review-editor-target").textContent.startsWith("Recreate deleted"), "deleted edit has no recovery affordance");
  save();
  await waitFor(() => articleWithBody(recoveredBody), "deleted annotation recreation");

  overlay.dispatchEvent(new PointerEvent("pointerdown", {
    bubbles: true,
    button: 0,
    pointerId: 42,
    clientX: rect.left + rect.width * 0.55,
    clientY: rect.top + rect.height * 0.45,
  }));
  document.dispatchEvent(new PointerEvent("pointermove", {
    bubbles: true,
    pointerId: 42,
    clientX: rect.left + rect.width * 0.75,
    clientY: rect.top + rect.height * 0.65,
  }));
  document.dispatchEvent(new PointerEvent("pointerup", {
    bubbles: true,
    pointerId: 42,
    clientX: rect.left + rect.width * 0.75,
    clientY: rect.top + rect.height * 0.65,
  }));
  await waitFor(() => form.classList.contains("is-open"), "region editor");
  const regionBody = `${unique}-region`;
  enterBody(regionBody);
  save();
  await waitFor(() => articleWithBody(regionBody), "region annotation create");
  const afterRegion = await snapshot();
  assert(afterRegion.annotations.find((item) => item.body === regionBody)?.target.type === "region", "region target was not persisted");

  let cleanup = afterRegion;
  for (const item of cleanup.annotations.filter((annotation) => annotation.body.startsWith(unique))) {
    const response = await mutate({ operation: "delete", revision: cleanup.revision, id: item.id });
    assert(response.status === 200, "smoke cleanup failed");
    cleanup = await response.json();
  }
  document.querySelector('[aria-label="Reload annotations"]').click();
  await waitFor(() => !Array.from(document.querySelectorAll(".sideshow-review-body")).some((node) => node.textContent.startsWith(unique)), "smoke cleanup refresh");

  return JSON.stringify({
    ok: true,
    operations: ["create", "edit", "resolve", "orphan", "conflict-recover", "region", "delete"],
    final_revision: cleanup.revision,
  });
})()
