import { runRenderer } from './runner.js';

// Pre-analyze the active request batch, then start only its required modules.
// Module promises are shared across blocks; one engine failure stays local.
const loaders = {
  mermaid: () => import('./mermaid-engine.js'),
  d2: () => import('@terrastruct/d2'),
  pikchr: () => import('pikchr-wasm'),
  vega: () => import('vega'),
  dot: () => import('@viz-js/viz'),
  chen: async () => ({ ...await import('@viz-js/viz'), ...await import('./chen.js') }),
  salt: () => import('./salt.js'),
  plantuml: async () => {
    window.PLANTUML_STDLIB_BASE = './';
    await Promise.all([import('./themes.js'), import('./c4.min.js')]);
    return import('./plantuml.js');
  },
  flowchart: () => import('flowchart.js'),
  wavedrom: async () => {
    const [wave, json] = await Promise.all([import('wavedrom'), import('json5')]);
    return { WaveDrom: wave.default, JSON5: json.default };
  },
  math: () => import('./math-engine.js'),
};
function engineFor({ kind, source }) {
  if (kind === 'zenuml') return 'mermaid';
  if (kind === 'inline-math') return 'math';
  if (kind === 'plantuml') {
    if (/^\s*@startchen\b/m.test(source)) return 'chen';
    if (/^\s*@startsalt\b/m.test(source) || /^\s*(?:@startuml\s*\n)?salt\b/m.test(source)) return 'salt';
    if (/^\s*@startdot\b/m.test(source)) return 'dot';
  }
  return kind;
}
const required = new Map();
for (const request of window.GLIM_REQUESTS) {
  const name = engineFor(request);
  if (!required.has(name)) {
    const loading = loaders[name] ? loaders[name]() : Promise.reject(new Error('未知图表类型'));
    loading.catch(() => {});
    required.set(name, loading);
  }
}
let viz, d2;
async function render(request, index) {
  const host = document.getElementById('output');
  host.replaceChildren();
  const { kind, source } = request;
  const engine = await required.get(engineFor(request));
  switch (kind) {
    case 'd2': {
      d2 ??= new engine.D2();
      const result = await d2.compile(source);
      return d2.render(result.diagram, result.renderOptions);
    }
    case 'pikchr': {
      const pikchr = engine.default;
      await pikchr.loadWASM();
      const svg = pikchr.render(source);
      if (!svg.trim().startsWith('<svg')) throw new Error('Pikchr syntax error');
      return svg;
    }
    case 'vega': {
      const vega = engine;
      const spec = JSON.parse(source);
      const loader = { load: async () => { throw new Error('External Vega resources are disabled'); }, sanitize: async () => { throw new Error('External Vega resources are disabled'); } };
      const view = new vega.View(vega.parse(spec), { renderer: 'none', loader });
      try { return await view.toSVG(); } finally { view.finalize(); }
    }
    case 'zenuml':
    case 'mermaid': {
      return engine.renderMermaid(request, index);
    }
    case 'dot':
      viz ??= await engine.instance();
      return viz.renderString(source, { format: 'svg' });
    case 'plantuml': {
      if (/^\s*@startchen\b/m.test(source)) {
        viz ??= await engine.instance();
        return viz.renderString(engine.chenToDot(source), { format: 'svg' });
      }
      if (/^\s*@startsalt\b/m.test(source) || /^\s*(?:@startuml\s*\n)?salt\b/m.test(source)) {
        return engine.renderSalt(source);
      }
      if (/^\s*@startdot\b/m.test(source)) {
        viz ??= await engine.instance();
        return viz.renderString(source.replace(/^\s*@startdot[^\n]*\n/m, '').replace(/^\s*@enddot\s*$/m, ''), { format: 'svg' });
      }
      if (/^\s*!include(?:url|_once|_many)?\s+(?!<)/mi.test(source)) {
        throw new Error('离线预览只支持随应用提供的 PlantUML 标准库 include');
      }
      const wrapped = /^\s*@start\w+/m.test(source) ? source : `@startuml\n${source}\n@enduml`;
      const svg = await new Promise((resolve, reject) => engine.renderToString(wrapped.split(/\r?\n/), resolve, reject));
      const document = new DOMParser().parseFromString(svg, 'image/svg+xml');
      const message = document.documentElement.textContent || '';
      if (/Diagram not supported by this release of PlantUML|Syntax Error\?|Syntax Error|Cannot include|Cannot load theme/i.test(message)) {
        throw new Error(message.trim().slice(0, 1200));
      }
      return svg;
    }
    case 'flowchart': {
      const flowchart = engine.default;
      host.id = `flowchart-${index}`;
      try {
        flowchart.parse(source).drawSVG(host.id);
        const svg = host.querySelector('svg');
        if (!svg) throw new Error('流程图没有可显示的节点');
        return svg.outerHTML;
      } finally { host.id = 'output'; }
    }
    case 'wavedrom': {
      const { WaveDrom, JSON5 } = engine;
      window.WaveSkin = WaveDrom.waveSkin;
      const container = document.createElement('div');
      container.id = `wave-${index}`;
      host.appendChild(container);
      WaveDrom.renderWaveForm(index, JSON5.parse(source), 'wave-');
      const svg = container.querySelector('svg');
      if (!svg) throw new Error('WaveDrom 没有可显示的波形');
      return svg.outerHTML;
    }
    case 'math':
    case 'inline-math': {
      return engine.renderMath(request);
    }
    default: throw new Error('未知图表类型');
  }
}

runRenderer(render);
