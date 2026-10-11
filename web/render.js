// Replays the core's draw list onto a canvas. It holds no rule and no
// number: op codes, colours, shapes and scales all come from the core's
// assets (tool_core::render::assets_json), sent once.
//
//   const renderer = createRenderer(canvas, assets);
//   renderer.setImage(assets.images.terrain, width, height, rgbaBytes);
//   renderer.draw(drawList);   // an Int32Array from the core, every frame

function toPath(polygons) {
  const path = new Path2D();
  for (const polygon of polygons) {
    const [first, ...rest] = polygon;
    path.moveTo(first[0], first[1]);
    for (const point of rest) {
      path.lineTo(point[0], point[1]);
    }
    path.closePath();
  }
  return path;
}

export function createRenderer(canvas, assets) {
  const ctx = canvas.getContext("2d");
  const palette = assets.palette;
  const paths = assets.shapes.map(toPath);
  const scales = assets.scales;
  const base = assets.base;
  const images = [];
  let list = null;
  let at = 0;

  const next = () => {
    const value = list[at];
    at += 1;
    return value;
  };

  const toBase = () => ctx.setTransform(base, 0, 0, base, 0, 0);

  function tracePoints() {
    const count = next();
    ctx.beginPath();
    for (let k = 0; k < count; k += 1) {
      const x = next();
      const y = next();
      ctx.lineTo(x, y);
    }
  }

  const handlers = [];
  handlers[assets.ops.clear] = () => {
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = palette[next()];
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    toBase();
  };
  handlers[assets.ops.image] = () => {
    const image = images[next()];
    const sx = next();
    const sy = next();
    const sw = next();
    const sh = next();
    const dx = next();
    const dy = next();
    const dw = next();
    const dh = next();
    if (image) {
      ctx.drawImage(image, sx, sy, sw, sh, dx, dy, dw, dh);
    }
  };
  handlers[assets.ops.shape] = () => {
    const path = paths[next()];
    const x = next();
    const y = next();
    const scale = scales[next()];
    ctx.fillStyle = palette[next()];
    ctx.strokeStyle = palette[next()];
    ctx.lineWidth = next();
    ctx.transform(scale, 0, 0, scale, x, y);
    ctx.stroke(path);
    ctx.fill(path);
    toBase();
  };
  handlers[assets.ops.fill_shape] = () => {
    const path = paths[next()];
    const x = next();
    const y = next();
    const scale = scales[next()];
    ctx.fillStyle = palette[next()];
    ctx.transform(scale, 0, 0, scale, x, y);
    ctx.fill(path);
    toBase();
  };
  handlers[assets.ops.poly] = () => {
    ctx.fillStyle = palette[next()];
    tracePoints();
    ctx.closePath();
    ctx.fill();
  };
  handlers[assets.ops.outline] = () => {
    ctx.strokeStyle = palette[next()];
    ctx.lineWidth = next();
    tracePoints();
    ctx.closePath();
    ctx.stroke();
  };
  handlers[assets.ops.line] = () => {
    ctx.strokeStyle = palette[next()];
    ctx.lineWidth = next();
    tracePoints();
    ctx.stroke();
  };

  // Keeps an RGBA image from the core as an offscreen canvas, by id.
  function setImage(id, width, height, bytes) {
    const pixels = new Uint8ClampedArray(bytes.buffer, bytes.byteOffset, bytes.length);
    const data = new ImageData(pixels, width, height);
    let image = images[id];
    if (!image || image.width !== width || image.height !== height) {
      image = document.createElement("canvas");
      image.width = width;
      image.height = height;
      images[id] = image;
    }
    image.getContext("2d").putImageData(data, 0, 0);
  }

  function draw(drawList) {
    list = drawList;
    at = 0;
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "low";
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    toBase();
    while (at < list.length) {
      handlers[next()]();
    }
  }

  return { setImage, draw };
}
