import fragmentSource from "./liquid-metal.frag?raw";

const vertexSource = `
  attribute vec2 position;
  varying vec2 vUv;
  void main() {
    vUv = position * 0.5 + 0.5;
    gl_Position = vec4(position, 0.0, 1.0);
  }
`;

function compile(gl: WebGLRenderingContext, type: number, source: string) {
  const shader = gl.createShader(type);
  if (!shader) return null;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (gl.getShaderParameter(shader, gl.COMPILE_STATUS)) return shader;
  gl.deleteShader(shader);
  return null;
}

function watchPointer(canvas: HTMLCanvasElement, move: (x: number, y: number) => void, leave: () => void) {
  // The caption and the canvas share the same hit area.
  const panel = canvas.closest("figure") ?? canvas;
  function onMove(event: PointerEvent) {
    if (event.pointerType !== "mouse") return;
    const bounds = canvas.getBoundingClientRect();
    if (bounds.width && bounds.height) {
      move(
        Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width)),
        Math.max(0, Math.min(1, (event.clientY - bounds.top) / bounds.height)),
      );
    }
  }
  function onLeave(event: PointerEvent) {
    if (event.pointerType === "mouse") leave();
  }
  const listeners = new AbortController();
  const options = { signal: listeners.signal, passive: true };
  panel.addEventListener("pointerenter", onMove, options);
  panel.addEventListener("pointermove", onMove, options);
  panel.addEventListener("pointerleave", onLeave, options);
  panel.addEventListener("pointercancel", onLeave, options);
  window.addEventListener("blur", leave, options);
  return () => listeners.abort();
}

function reflectOnVideo(canvas: HTMLCanvasElement) {
  const style = (canvas.parentElement ?? canvas).style;
  const stop = watchPointer(canvas, (x, y) => {
    style.setProperty("--pointer-x", x * 100 + "%");
    style.setProperty("--pointer-y", y * 100 + "%");
    style.setProperty("--pointer-strength", "1");
  }, () => style.removeProperty("--pointer-strength"));
  return () => {
    stop();
    style.removeProperty("--pointer-x");
    style.removeProperty("--pointer-y");
    style.removeProperty("--pointer-strength");
  };
}

export function mountLiquidMetal(canvas: HTMLCanvasElement, setVideo: (enabled: boolean) => void) {
  if (typeof window.matchMedia !== "function") return;
  const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
  let dispose: (() => void) | undefined;

  function sync() {
    dispose?.();
    dispose = undefined;
    delete canvas.dataset.ready;
    if (!motion.matches && typeof WebGLRenderingContext !== "undefined" &&
      typeof ResizeObserver !== "undefined" && typeof IntersectionObserver !== "undefined") {
      const gl = canvas.getContext("webgl", { alpha: false, antialias: false, depth: false, stencil: false });
      if (gl && !gl.isContextLost()) dispose = animate(canvas, gl);
    }
    const useVideo = !motion.matches && !dispose;
    setVideo(useVideo);
    if (useVideo) dispose = reflectOnVideo(canvas);
  }

  function onContextLost(event: Event) {
    event.preventDefault();
    sync();
  }

  sync();
  motion.addEventListener("change", sync);
  canvas.addEventListener("webglcontextlost", onContextLost);
  canvas.addEventListener("webglcontextrestored", sync);
  return () => {
    motion.removeEventListener("change", sync);
    canvas.removeEventListener("webglcontextlost", onContextLost);
    canvas.removeEventListener("webglcontextrestored", sync);
    dispose?.();
  };
}

function animate(canvas: HTMLCanvasElement, gl: WebGLRenderingContext) {
  const vertex = compile(gl, gl.VERTEX_SHADER, vertexSource);
  const fragment = compile(gl, gl.FRAGMENT_SHADER, fragmentSource);
  const program = gl.createProgram();
  const buffer = gl.createBuffer();
  function release() {
    gl.deleteBuffer(buffer);
    gl.deleteProgram(program);
    gl.deleteShader(vertex);
    gl.deleteShader(fragment);
  }
  if (!vertex || !fragment || !program || !buffer) { release(); return; }
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) { release(); return; }

  gl.useProgram(program);
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const position = gl.getAttribLocation(program, "position");
  gl.enableVertexAttribArray(position);
  gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);

  const resolution = gl.getUniformLocation(program, "uResolution");
  const clock = gl.getUniformLocation(program, "uTime");
  const pointer = gl.getUniformLocation(program, "uPointer");
  let x = 0.5, y = 0.5, targetX = x, targetY = y;
  let hovering = false;
  let strength = 0;
  const stopPointer = watchPointer(canvas, (nextX, nextY) => {
    if (!hovering && strength < 0.01) { x = nextX; y = nextY; }
    targetX = nextX;
    targetY = nextY;
    hovering = true;
  }, () => { hovering = false; });

  let visible = false;
  let frame = 0;
  let elapsed = 0;
  let previous: number | undefined;

  function draw() {
    gl.uniform1f(clock, elapsed);
    gl.uniform3f(pointer, x, y, strength);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    if (!canvas.hasAttribute("data-ready")) canvas.dataset.ready = "";
  }

  function tick(now: number) {
    frame = requestAnimationFrame(tick);
    if (previous !== undefined) {
      const delta = now - previous;
      if (delta < 1000 / 30) return;
      elapsed += delta / 1000;
      const follow = 1 - Math.exp(-delta / 95);
      x += (targetX - x) * follow;
      y += (targetY - y) * follow;
      strength += ((hovering ? 1 : 0) - strength) * (1 - Math.exp(-delta / (hovering ? 130 : 230)));
      if (!hovering && strength < 0.001) strength = 0;
    }
    previous = now;
    draw();
  }

  function updatePlayback() {
    cancelAnimationFrame(frame);
    previous = undefined;
    if (visible && !document.hidden) frame = requestAnimationFrame(tick);
    else { hovering = false; strength = 0; }
  }

  function resize() {
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (!w || !h) return;
    const density = Math.min(devicePixelRatio, 2, 1200 / Math.max(w, h));
    canvas.width = Math.max(1, Math.round(w * density));
    canvas.height = Math.max(1, Math.round(h * density));
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.uniform2f(resolution, w, h);
    draw();
  }

  const sizes = new ResizeObserver(resize);
  const visibility = new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    updatePlayback();
  });
  sizes.observe(canvas);
  visibility.observe(canvas);
  document.addEventListener("visibilitychange", updatePlayback);
  return () => {
    stopPointer();
    cancelAnimationFrame(frame);
    sizes.disconnect();
    visibility.disconnect();
    document.removeEventListener("visibilitychange", updatePlayback);
    release();
  };
}
