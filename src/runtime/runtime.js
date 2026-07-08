(() => {
  "use strict";
  const marker = "sideshow-runtime-v1";
  const slides = Array.from(document.querySelectorAll(".slide"));
  let index = 0;
  let digits = "";
  const reduceMotion = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  function scale() {
    const s = Math.min(window.innerWidth / 1920, window.innerHeight / 1080);
    document.documentElement.style.setProperty("--sideshow-scale", String(s));
  }

  function steps(slide) {
    return Array.from(slide.querySelectorAll("[data-step]")).sort((a, b) => {
      const av = Number(a.dataset.step || "9999");
      const bv = Number(b.dataset.step || "9999");
      return av === bv ? 0 : av - bv;
    });
  }

  function apply() {
    slides.forEach((slide, i) => {
      slide.classList.toggle("is-active", i === index);
      if (i !== index) steps(slide).forEach((el) => el.classList.remove("is-revealed"));
      updateVideos(slide, i === index);
    });
    history.replaceState(null, "", `#${index + 1}`);
  }

  function initVideos() {
    slides.forEach((slide) => Array.from(slide.querySelectorAll("video")).forEach((video) => {
      video.muted = true;
      video.loop = true;
      video.playsInline = true;
      video.setAttribute("muted", "");
      video.setAttribute("loop", "");
      video.setAttribute("playsinline", "");
    }));
  }

  function updateVideos(slide, active) {
    Array.from(slide.querySelectorAll("video")).forEach((video) => {
      if (active && !reduceMotion) {
        const p = video.play();
        if (p && typeof p.catch === "function") p.catch(() => {});
      } else {
        video.pause();
        video.currentTime = 0;
      }
    });
  }

  function go(n) {
    index = Math.max(0, Math.min(slides.length - 1, n));
    apply();
  }

  function goto(n) { go(Number(n) - 1); }

  function next() {
    const hidden = steps(slides[index]).find((el) => !el.classList.contains("is-revealed"));
    if (hidden) { hidden.classList.add("is-revealed"); return; }
    go(index + 1);
  }

  function prev() {
    const shown = steps(slides[index]).filter((el) => el.classList.contains("is-revealed"));
    if (shown.length) { shown[shown.length - 1].classList.remove("is-revealed"); return; }
    go(index - 1);
  }

  function count() { return slides.length; }

  function notes(n) {
    const slide = slides[Math.max(0, Math.min(slides.length - 1, Number(n) - 1))];
    const tmpl = slide && slide.querySelector("template[data-notes]");
    return tmpl ? tmpl.textContent.trim() : null;
  }

  function auditSlide(slide, i) {
    const wasActive = slide.classList.contains("is-active");
    const oldVisibility = slide.style.visibility;
    const oldOpacity = slide.style.opacity;
    const oldPointerEvents = slide.style.pointerEvents;
    if (!wasActive) {
      slide.style.visibility = "visible";
      slide.style.opacity = "0";
      slide.style.pointerEvents = "none";
    }
    const kids = Array.from(slide.children).filter((el) => !el.matches("template[data-notes]"));
    let right = slide.scrollWidth;
    let bottom = slide.scrollHeight;
    const base = slide.getBoundingClientRect();
    kids.forEach((el) => {
      const r = el.getBoundingClientRect();
      right = Math.max(right, (r.right - base.left) / (base.width / 1920 || 1));
      bottom = Math.max(bottom, (r.bottom - base.top) / (base.height / 1080 || 1));
    });
    if (!wasActive) {
      slide.style.visibility = oldVisibility;
      slide.style.opacity = oldOpacity;
      slide.style.pointerEvents = oldPointerEvents;
    }
    return {
      index: i + 1,
      id: slide.id || null,
      src: slide.dataset.src || null,
      overflow: { x: Math.max(0, Math.ceil(right - 1920)), y: Math.max(0, Math.ceil(bottom - 1080)) },
      steps: steps(slide).length,
      hasNotes: !!slide.querySelector("template[data-notes]"),
      brokenImages: Array.from(slide.querySelectorAll("img")).filter((img) => img.complete && img.naturalWidth === 0).map((img) => img.getAttribute("src") || "")
    };
  }

  function audit() {
    const stage = document.getElementById("stage");
    const oldTransition = slides.map((s) => s.style.transition);
    slides.forEach((s) => { s.style.transition = "none"; });
    const out = { canvas: { w: 1920, h: 1080 }, slides: slides.map(auditSlide) };
    slides.forEach((s, i) => { s.style.transition = oldTransition[i]; });
    if (stage) stage.offsetHeight;
    return out;
  }

  window.addEventListener("resize", scale);
  window.addEventListener("hashchange", () => {
    const n = Number(location.hash.slice(1));
    if (Number.isFinite(n) && n > 0) go(n - 1);
  });
  document.addEventListener("keydown", (event) => {
    if (/^[0-9]$/.test(event.key)) { digits += event.key; return; }
    if (event.key === "Enter" && digits) { go(Number(digits) - 1); digits = ""; return; }
    digits = "";
    if (["ArrowRight", "ArrowDown", " ", "PageDown"].includes(event.key)) { event.preventDefault(); next(); }
    if (["ArrowLeft", "ArrowUp", "PageUp"].includes(event.key)) { event.preventDefault(); prev(); }
    if (event.key === "Home") go(0);
    if (event.key === "End") go(slides.length - 1);
  });
  scale();
  initVideos();
  const start = Number(location.hash.slice(1));
  if (Number.isFinite(start) && start > 0) index = Math.min(slides.length - 1, start - 1);
  apply();
  // API markers for static checks: sideshow.goto sideshow.audit
  window.sideshow = { marker, goto, go: goto, next, prev, count, notes, audit };
})();
