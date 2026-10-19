import mermaid from 'mermaid';

// Diagram source is data, never executable JavaScript. All engine resources
// are served by the private, in-memory protocol; CSP blocks remote resources.
mermaid.initialize({
  startOnLoad: false,
  securityLevel: 'strict',
  theme: 'default',
  fontFamily: 'Arial, sans-serif',
  flowchart: { htmlLabels: false },
  journey: { textPlacement: 'svg' },
  htmlLabels: false,
  maxTextSize: window.GLIM_LIMITS.DIAGRAM_SOURCE_BYTES,
  maxEdges: window.GLIM_LIMITS.DIAGRAM_MERMAID_EDGES,
});
let externalDiagramsReady;

export async function renderMermaid({ kind, source }, index) {
  document.getElementById('output').replaceChildren();
  if (kind === 'zenuml' || /^\s*zenuml\b/.test(source)) {
    externalDiagramsReady ??= import('@mermaid-js/mermaid-zenuml').then(({ default: zenuml }) => mermaid.registerExternalDiagrams([zenuml]));
    await externalDiagramsReady;
  }
  const input = kind === 'zenuml' && !/^\s*zenuml\b/.test(source) ? `zenuml\n${source}` : source;
  const result = await mermaid.render(`glim-diagram-${index}`, input);
  return result.svg;
}
