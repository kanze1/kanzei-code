// R-285 金色神经流。
// 顶层声明作为所有 classic script 的统一事件入口；初始化前保持可安全调用。
let neuralFlowEmit = null;
// 视觉层只消费真实运行事件；Canvas 丢帧、隐藏或关闭不会反向改变任何业务状态。
(() => {
  const chatCanvas = $("neural-flow-chat");
  const memoryCanvas = $("neural-flow-memory");
  const stateLabel = $("memory-flow-state");
  const reducedMotion = typeof window.matchMedia === "function"
    && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const fields = [];
  let frameHandle = 0;
  let stateTimer = null;
  let lastStreamPulseAt = 0;

  function flowNow() {
    return typeof performance !== "undefined" && typeof performance.now === "function" ? performance.now() : Date.now();
  }

  function seededRandom(seed) {
    let value = seed >>> 0;
    return () => {
      value += 0x6d2b79f5;
      let mixed = value;
      mixed = Math.imul(mixed ^ (mixed >>> 15), mixed | 1);
      mixed ^= mixed + Math.imul(mixed ^ (mixed >>> 7), mixed | 61);
      return ((mixed ^ (mixed >>> 14)) >>> 0) / 4294967296;
    };
  }

  function themeColor(name, fallback) {
    if (typeof getComputedStyle !== "function") return fallback;
    const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    return value || fallback;
  }

  function quadraticPoint(a, b, bend, progress) {
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const length = Math.max(1, Math.hypot(dx, dy));
    const mx = (a.x + b.x) / 2 - (dy / length) * bend;
    const my = (a.y + b.y) / 2 + (dx / length) * bend;
    const inv = 1 - progress;
    return {
      x: inv * inv * a.x + 2 * inv * progress * mx + progress * progress * b.x,
      y: inv * inv * a.y + 2 * inv * progress * my + progress * progress * b.y,
      mx,
      my,
    };
  }

  class NeuralField {
    constructor(canvas, variant, seed) {
      this.canvas = canvas;
      this.variant = variant;
      this.random = seededRandom(seed);
      this.context = typeof canvas?.getContext === "function" ? canvas.getContext("2d") : null;
      this.width = 0;
      this.height = 0;
      this.nodes = [];
      this.edges = [];
      this.pulses = [];
      this.bursts = [];
      this.energy = variant === "memory" ? 0.22 : 0.12;
      this.lastFrameAt = 0;
      if (!this.context) return;
      this.resize();
      if (typeof ResizeObserver === "function") {
        this.resizeObserver = new ResizeObserver(() => {
          this.resize();
          if (reducedMotion) this.draw(flowNow());
        });
        this.resizeObserver.observe(canvas);
      }
    }

    resize() {
      const rect = this.canvas.getBoundingClientRect();
      const width = Math.max(1, rect.width || this.canvas.clientWidth || 1);
      const height = Math.max(1, rect.height || this.canvas.clientHeight || 1);
      const pixelRatio = Math.min(1.75, Math.max(1, window.devicePixelRatio || 1));
      const pixelWidth = Math.round(width * pixelRatio);
      const pixelHeight = Math.round(height * pixelRatio);
      if (this.canvas.width !== pixelWidth || this.canvas.height !== pixelHeight) {
        this.canvas.width = pixelWidth;
        this.canvas.height = pixelHeight;
      }
      this.context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
      const changed = Math.abs(width - this.width) > 1 || Math.abs(height - this.height) > 1;
      this.width = width;
      this.height = height;
      if (changed || !this.nodes.length) this.buildTopology();
    }

    buildTopology() {
      const count = this.variant === "memory" ? 46 : 38;
      this.nodes = [];
      for (let index = 0; index < count; index += 1) {
        const focus = this.variant === "chat" && index < 29;
        const x = this.variant === "memory"
          ? 0.05 + this.random() * 0.9
          : focus ? 0.48 + this.random() * 0.56 : 0.12 + this.random() * 0.46;
        const y = this.variant === "memory"
          ? 0.16 + this.random() * 0.7
          : 0.06 + this.random() * 0.88;
        this.nodes.push({
          x: x * this.width,
          y: y * this.height,
          radius: 0.7 + this.random() * 1.45,
          phase: this.random() * Math.PI * 2,
          weight: 0.35 + this.random() * 0.65,
        });
      }
      this.edges = [];
      const seen = new Set();
      this.nodes.forEach((node, index) => {
        const nearest = this.nodes
          .map((other, otherIndex) => ({
            otherIndex,
            distance: otherIndex === index ? Number.POSITIVE_INFINITY : Math.hypot(other.x - node.x, other.y - node.y),
          }))
          .sort((a, b) => a.distance - b.distance)
          .slice(0, index % 5 === 0 ? 3 : 2);
        nearest.forEach(({ otherIndex, distance }) => {
          const key = index < otherIndex ? `${index}:${otherIndex}` : `${otherIndex}:${index}`;
          if (seen.has(key)) return;
          seen.add(key);
          this.edges.push({
            from: index,
            to: otherIndex,
            bend: (this.random() - 0.5) * Math.min(34, distance * 0.24),
            weight: 0.25 + this.random() * 0.75,
            phase: this.random(),
            speed: 0.000035 + this.random() * 0.000035,
            signal: this.random() > (this.variant === "memory" ? 0.82 : 0.9),
          });
        });
      });
    }

    activeInView() {
      const view = this.canvas.closest?.(".view");
      return !view || view.classList.contains("active");
    }

    trigger(kind, intensity = 0.7) {
      if (!this.context) return;
      const now = flowNow();
      const strength = Math.max(0.15, Math.min(1, intensity));
      this.energy = Math.max(this.energy, strength);
      const count = kind === "stream" ? 1 : kind === "crystal" ? 8 : kind === "error" ? 3 : 5;
      for (let index = 0; index < count; index += 1) {
        const edgeIndex = this.pickEdge(kind, index);
        this.pulses.push({
          edgeIndex,
          reverse: kind === "recall" || (kind === "crystal" && index % 2 === 0),
          startedAt: now + index * (kind === "crystal" ? 54 : 68),
          duration: 520 + this.random() * 620,
          strength,
          kind,
        });
      }
      if (["crystal", "complete", "error"].includes(kind)) {
        const nodeIndex = this.pickAnchor(kind);
        this.bursts.push({ nodeIndex, startedAt: now + (kind === "crystal" ? 260 : 80), kind, strength });
      }
      if (reducedMotion) this.draw(now);
    }

    pickEdge(kind, offset) {
      if (!this.edges.length) return 0;
      const ranked = this.edges.map((edge, index) => {
        const from = this.nodes[edge.from];
        const to = this.nodes[edge.to];
        const centerX = (from.x + to.x) / 2 / Math.max(1, this.width);
        const centerY = (from.y + to.y) / 2 / Math.max(1, this.height);
        let score = this.random() * 0.35;
        if (kind === "recall") score += centerX * 0.8 + (1 - centerY) * 0.2;
        else if (kind === "write" || kind === "crystal") score += centerX * 0.55 + centerY * 0.25;
        else if (kind === "run") score += (1 - centerX) * 0.45 + centerY * 0.2;
        else score += edge.weight * 0.45;
        return { index, score };
      }).sort((a, b) => b.score - a.score);
      return ranked[offset % Math.min(ranked.length, 12)].index;
    }

    pickAnchor(kind) {
      const ranked = this.nodes.map((node, index) => {
        const nx = node.x / Math.max(1, this.width);
        const ny = node.y / Math.max(1, this.height);
        const score = kind === "error"
          ? Math.abs(nx - 0.68) + Math.abs(ny - 0.5)
          : Math.abs(nx - 0.78) + Math.abs(ny - 0.42);
        return { index, score };
      }).sort((a, b) => a.score - b.score);
      return ranked[0]?.index ?? 0;
    }

    draw(now) {
      if (!this.context || !this.width || !this.height) return;
      const ctx = this.context;
      ctx.clearRect(0, 0, this.width, this.height);
      const base = themeColor("--memory-flow", "#c9962e");
      const hot = themeColor("--memory-flow-hot", "#f0d58c");
      const error = themeColor("--err", "#d0684e");
      const isMemory = this.variant === "memory";
      const idleAlpha = isMemory ? 0.22 : 0.075;
      const breathing = reducedMotion ? 0.72 : 0.74 + Math.sin(now / 1500) * 0.14;
      const energy = Math.max(this.variant === "memory" ? 0.18 : 0.08, this.energy);

      ctx.lineCap = "round";
      for (const edge of this.edges) {
        const from = this.nodes[edge.from];
        const to = this.nodes[edge.to];
        const point = quadraticPoint(from, to, edge.bend, 0.5);
        ctx.save();
        ctx.beginPath();
        ctx.moveTo(from.x, from.y);
        ctx.quadraticCurveTo(point.mx, point.my, to.x, to.y);
        ctx.strokeStyle = edge.weight > 0.78 ? hot : base;
        ctx.globalAlpha = idleAlpha * (0.55 + edge.weight * 0.45) * (0.75 + energy * 0.5);
        ctx.lineWidth = (isMemory ? 0.72 : 0.48) + edge.weight * (isMemory ? 0.52 : 0.42);
        if (edge.weight > 0.72) {
          ctx.shadowColor = base;
          ctx.shadowBlur = isMemory ? 5 : 2.5;
        }
        ctx.stroke();
        ctx.restore();

        // 静息时仍有少量定向流光，让“神经流”不依赖业务事件也能一眼读出流向。
        if (edge.signal) {
          const ambientProgress = reducedMotion ? edge.phase : (edge.phase + now * edge.speed) % 1;
          const ambientFade = Math.sin(ambientProgress * Math.PI);
          const tailProgress = Math.max(0, ambientProgress - 0.045);
          const tail = quadraticPoint(from, to, edge.bend, tailProgress);
          const head = quadraticPoint(from, to, edge.bend, ambientProgress);
          ctx.save();
          ctx.beginPath();
          ctx.moveTo(tail.x, tail.y);
          ctx.lineTo(head.x, head.y);
          ctx.strokeStyle = hot;
          ctx.lineWidth = isMemory ? 1.6 : 0.9;
          ctx.globalAlpha = (isMemory ? 0.5 : 0.18) * (0.68 + ambientFade * 0.32);
          ctx.shadowColor = hot;
          ctx.shadowBlur = isMemory ? 10 : 5;
          ctx.stroke();
          ctx.beginPath();
          ctx.arc(head.x, head.y, (isMemory ? 1.05 : 0.7) + edge.weight * 0.5, 0, Math.PI * 2);
          ctx.fillStyle = hot;
          ctx.globalAlpha = (isMemory ? 0.72 : 0.3) * (0.72 + ambientFade * 0.28);
          ctx.fill();
          ctx.restore();
        }
      }

      for (const node of this.nodes) {
        const pulse = reducedMotion ? 1 : 0.82 + Math.sin(now / 1250 + node.phase) * 0.18;
        ctx.save();
        ctx.beginPath();
        ctx.arc(node.x, node.y, node.radius * pulse, 0, Math.PI * 2);
        ctx.fillStyle = node.weight > 0.76 ? hot : base;
        ctx.globalAlpha = (idleAlpha * 1.55 + energy * 0.055) * breathing;
        if (node.weight > 0.72) {
          ctx.shadowColor = node.weight > 0.76 ? hot : base;
          ctx.shadowBlur = isMemory ? 8 : 4;
        }
        ctx.fill();
        ctx.restore();
      }

      const nextPulses = [];
      for (const pulse of this.pulses) {
        const progress = (now - pulse.startedAt) / pulse.duration;
        if (progress < 0) { nextPulses.push(pulse); continue; }
        if (progress >= 1) continue;
        const edge = this.edges[pulse.edgeIndex];
        if (!edge) continue;
        const from = this.nodes[pulse.reverse ? edge.to : edge.from];
        const to = this.nodes[pulse.reverse ? edge.from : edge.to];
        const point = quadraticPoint(from, to, pulse.reverse ? -edge.bend : edge.bend, progress);
        const color = pulse.kind === "error" ? error : pulse.kind === "complete" || pulse.kind === "crystal" ? hot : base;
        const fade = Math.sin(progress * Math.PI);
        ctx.save();
        ctx.beginPath();
        ctx.moveTo(from.x, from.y);
        const edgeMidpoint = quadraticPoint(from, to, pulse.reverse ? -edge.bend : edge.bend, 0.5);
        ctx.quadraticCurveTo(edgeMidpoint.mx, edgeMidpoint.my, to.x, to.y);
        ctx.strokeStyle = color;
        ctx.lineWidth = 0.8 + pulse.strength * 0.7;
        ctx.globalAlpha = (0.1 + pulse.strength * 0.12) * fade;
        ctx.shadowColor = color;
        ctx.shadowBlur = 5 + pulse.strength * 6;
        ctx.stroke();

        const trailSteps = 5;
        for (let step = trailSteps; step >= 1; step -= 1) {
          const trailProgress = Math.max(0, progress - step * 0.026);
          const trailPoint = quadraticPoint(from, to, pulse.reverse ? -edge.bend : edge.bend, trailProgress);
          ctx.beginPath();
          ctx.arc(trailPoint.x, trailPoint.y, 0.55 + pulse.strength * 0.6, 0, Math.PI * 2);
          ctx.fillStyle = color;
          ctx.globalAlpha = fade * (1 - step / (trailSteps + 1)) * 0.52;
          ctx.fill();
        }
        ctx.beginPath();
        ctx.arc(point.x, point.y, 1.45 + pulse.strength * 2.35, 0, Math.PI * 2);
        ctx.fillStyle = color;
        ctx.globalAlpha = 0.68 + fade * 0.32;
        ctx.shadowColor = color;
        ctx.shadowBlur = 12 + pulse.strength * 18;
        ctx.fill();
        ctx.restore();
        nextPulses.push(pulse);
      }
      this.pulses = nextPulses;

      const nextBursts = [];
      for (const burst of this.bursts) {
        const progress = (now - burst.startedAt) / 1050;
        if (progress < 0) { nextBursts.push(burst); continue; }
        if (progress >= 1) continue;
        const node = this.nodes[burst.nodeIndex];
        const color = burst.kind === "error" ? error : hot;
        ctx.save();
        ctx.beginPath();
        ctx.arc(node.x, node.y, 5 + progress * 25 * burst.strength, 0, Math.PI * 2);
        ctx.strokeStyle = color;
        ctx.lineWidth = 1.45;
        ctx.globalAlpha = (1 - progress) * 0.86;
        ctx.shadowColor = color;
        ctx.shadowBlur = 18;
        ctx.stroke();
        ctx.restore();
        nextBursts.push(burst);
      }
      this.bursts = nextBursts;
      ctx.globalAlpha = 1;
      this.energy += ((this.variant === "memory" ? 0.18 : 0.08) - this.energy) * 0.026;
    }
  }

  function addField(canvas, variant, seed) {
    if (!canvas) return;
    const field = new NeuralField(canvas, variant, seed);
    if (field.context) fields.push(field);
  }

  addField(chatCanvas, "chat", 28501);
  addField(memoryCanvas, "memory", 28502);

  function setMemoryState(label, settleAfterMs = 0) {
    if (!stateLabel) return;
    stateLabel.textContent = t(label);
    clearTimeout(stateTimer);
    if (settleAfterMs > 0) {
      stateTimer = setTimeout(() => { stateLabel.textContent = t("静息"); }, settleAfterMs);
    }
  }

  const specs = {
    run_started: ["run", "运行中", 0.82],
    reasoning_active: ["stream", "运行中", 0.42],
    assistant_streaming: ["stream", "运行中", 0.32],
    tool_started: ["tool", "运行中", 0.64],
    tool_completed: ["complete", "收敛", 0.62],
    run_completed: ["complete", "收敛", 0.86],
    run_failed: ["error", "受阻", 0.9],
    run_stopped: ["error", "受阻", 0.55],
    context_compacted: ["crystal", "收敛", 0.72],
    memory_snapshot: ["ambient", null, 0.24],
    memory_search_started: ["recall", "检索中", 0.8],
    memory_search_completed: ["complete", "收敛", 0.68],
    memory_consolidation_started: ["write", "整理中", 0.88],
    memory_consolidation_partial: ["write", "整理中", 0.58],
    memory_consolidation_completed: ["crystal", "收敛", 1],
    memory_consolidation_failed: ["error", "受阻", 0.9],
    memory_candidate_discarded: ["error", "回收中", 0.52],
    memory_cleanup_started: ["write", "整理中", 0.72],
    memory_cleanup_completed: ["complete", "收敛", 0.72],
    memory_cleanup_failed: ["error", "受阻", 0.82],
    memory_search_failed: ["error", "受阻", 0.82],
    memory_recall_retrieved: ["recall", "检索中", 0.9],
    memory_recall_injected: ["recall", "注入中", 1],
    memory_candidate_created: ["write", "生长中", 0.86],
    memory_candidate_promoted: ["crystal", "收敛", 1],
    research_source_retrieved: ["recall", "检索中", 0.82],
    research_section_verified: ["crystal", "收敛", 0.9],
    voice_listening: ["recall", "运行中", 0.68],
    voice_speaking: ["complete", "运行中", 0.68],
    voice_interrupted: ["error", "受阻", 0.62],
  };

  neuralFlowEmit = (eventType, detail = {}) => {
    if (detail.session_id && activeSessionId && detail.session_id !== activeSessionId) return;
    if (eventType === "assistant_streaming") {
      const now = flowNow();
      if (now - lastStreamPulseAt < 180) return;
      lastStreamPulseAt = now;
    }
    let spec = specs[eventType];
    if (eventType === "tool_started") {
      const name = String(detail.tool_name ?? "").toLowerCase();
      if (name === "memory_search") spec = ["recall", "检索中", 0.9];
      else if (["memory_note", "memory_add", "memory_update"].includes(name)) spec = ["write", "生长中", 0.86];
    }
    if (eventType === "tool_completed" && detail.ok === false) spec = ["error", "受阻", 0.82];
    if (!spec) return;
    const [kind, label, intensity] = spec;
    if (label) setMemoryState(label, ["收敛", "受阻", "回收中"].includes(label) ? 1900 : 0);
    fields.forEach((field) => field.trigger(kind, detail.intensity ?? intensity));
  };

  function renderFrame(now) {
    if (!document.hidden) {
      for (const field of fields) {
        if (!field.activeInView()) continue;
        const active = field.pulses.length || field.bursts.length || field.energy > 0.24;
        const interval = active ? 16 : 42;
        if (now - field.lastFrameAt >= interval) {
          field.lastFrameAt = now;
          field.draw(now);
        }
      }
    }
    frameHandle = requestAnimationFrame(renderFrame);
  }

  if (fields.length) {
    if (reducedMotion) fields.forEach((field) => field.draw(flowNow()));
    else frameHandle = requestAnimationFrame(renderFrame);
  }

  window.addEventListener?.("beforeunload", () => {
    if (frameHandle) cancelAnimationFrame(frameHandle);
    fields.forEach((field) => field.resizeObserver?.disconnect());
  });
})();
