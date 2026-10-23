import mermaid from 'mermaid';

// Diagram source is data, never executable JavaScript. All engine resources
// are served by the private, in-memory protocol; CSP blocks remote resources.
mermaid.initialize({
  startOnLoad: false,
  securityLevel: 'strict',
  theme: 'base',
  fontFamily: '-apple-system, BlinkMacSystemFont, "Helvetica Neue", "PingFang SC", sans-serif',
  themeVariables: {
    primaryColor: '#e5f2ff',
    primaryTextColor: '#2385e8',
    primaryBorderColor: '#d3dce5',
    secondaryColor: '#f0f6fc',
    tertiaryColor: '#f7f9fc',
    lineColor: '#9299a1',
    textColor: '#263445',
    edgeLabelBackground: '#ffffff',
    clusterBkg: '#f7f9fc',
    clusterBorder: '#d3dce5',
    fontSize: '16px',
  },
  flowchart: {
    htmlLabels: false,
    curve: 'rounded',
    padding: 22,
    nodeSpacing: 48,
    rankSpacing: 72,
    wrappingWidth: 360,
    diagramPadding: 24,
  },
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
  if (result.diagramType === 'flowchart' || result.diagramType === 'flowchart-v2') {
    return polishFlowchart(result.svg);
  }
  return result.svg;
}

// Keep the output pure SVG for the native renderer. Only soften ordinary
// rectangles: diamonds, subroutines and explicitly rounded shapes retain their
// Mermaid geometry, and source-defined styles continue to take precedence.
function polishFlowchart(svg) {
  const root = new DOMParser().parseFromString(svg, 'image/svg+xml').documentElement;
  for (const rect of root.querySelectorAll('.node > rect.basic.label-container')) {
    if (Number(rect.getAttribute('rx')) > 0 || rect.hasAttribute('style') && /\br[xy]\s*:/.test(rect.getAttribute('style'))) continue;
    rect.setAttribute('rx', '14');
    rect.setAttribute('ry', '14');
  }
  for (const rect of root.querySelectorAll('.edgeLabel rect.background')) {
    const width = Number(rect.getAttribute('width'));
    const height = Number(rect.getAttribute('height'));
    if (!(width > 0 && height > 0)) continue;
    rect.setAttribute('x', String(Number(rect.getAttribute('x')) - 10));
    rect.setAttribute('y', String(Number(rect.getAttribute('y')) - 5));
    rect.setAttribute('width', String(width + 20));
    rect.setAttribute('height', String(height + 10));
    rect.setAttribute('rx', String((height + 10) / 2));
    rect.setAttribute('ry', String((height + 10) / 2));
    // Mermaid emits an inline `stroke: none` for SVG label backgrounds.
    rect.style.setProperty('stroke', '#2385e8');
    rect.style.setProperty('stroke-width', '1');
    rect.style.setProperty('fill', '#ffffff');
  }
  return new XMLSerializer().serializeToString(root);
}
