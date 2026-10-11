// The Benchmark page: the core runs the performance-test scenario and lays
// out every frame; this page passes timestamps and pointer input in, replays
// the draw list, and shows the panel text the core formats.
import init, { Bench, render_assets } from "./pkg/tool_shim.js";
import { createRenderer } from "./render.js";

const canvas = document.getElementById("view");
const panel = document.getElementById("panel");

function showPanel(rows) {
  const cells = [];
  for (const [label, value] of rows) {
    const dt = document.createElement("dt");
    dt.textContent = label;
    const dd = document.createElement("dd");
    dd.textContent = value;
    cells.push(dt, dd);
  }
  panel.replaceChildren(...cells);
}

async function start() {
  await init();
  canvas.width = canvas.clientWidth;
  canvas.height = canvas.clientHeight;
  const bench = new Bench(canvas.width, canvas.height, window.location.search);
  const assets = JSON.parse(render_assets());
  const renderer = createRenderer(canvas, assets);

  let stamp = null;
  function refreshTerrain() {
    const size = bench.terrain_size();
    renderer.setImage(assets.images.terrain, size[0], size[1], bench.terrain());
    stamp = bench.terrain_stamp();
  }
  refreshTerrain();

  const fogSize = bench.fog_size();
  function refreshFog() {
    renderer.setImage(assets.images.fog, fogSize[0], fogSize[1], bench.fog());
  }
  refreshFog();

  window.addEventListener("resize", () => {
    canvas.width = canvas.clientWidth;
    canvas.height = canvas.clientHeight;
    bench.resize(canvas.width, canvas.height);
  });
  canvas.addEventListener("wheel", (event) => {
    event.preventDefault();
    bench.wheel(event.deltaY, event.offsetX, event.offsetY);
  }, { passive: false });
  canvas.addEventListener("pointerdown", (event) => {
    canvas.setPointerCapture(event.pointerId);
    bench.drag_start(event.offsetX, event.offsetY);
  });
  canvas.addEventListener("pointermove", (event) => {
    bench.drag_move(event.offsetX, event.offsetY);
  });
  canvas.addEventListener("pointerup", () => bench.drag_end());

  function frame(now) {
    bench.frame_start(now);
    let ticked = false;
    while (bench.tick_due()) {
      const started = performance.now();
      bench.tick();
      bench.tick_done(started, performance.now());
      ticked = true;
    }
    if (ticked) {
      refreshFog();
    }
    if (bench.terrain_stamp() !== stamp) {
      refreshTerrain();
    }
    const started = performance.now();
    renderer.draw(bench.draw());
    bench.draw_done(started, performance.now());
    showPanel(JSON.parse(bench.panel()));
    document.body.dataset.ready = "true";
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
}

start();
