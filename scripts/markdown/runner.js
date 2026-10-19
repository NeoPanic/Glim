// Shared sanitization, limits, timeout and IPC for both renderer bundles.
function normalize(svg) {
  const root = new DOMParser().parseFromString(svg, 'image/svg+xml').documentElement;
  if (root.localName !== 'svg' || root.querySelector('parsererror')) {
    throw new Error('图表引擎没有返回有效 SVG');
  }
  root.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
  if ((!root.hasAttribute('width') || root.getAttribute('width').includes('%')) && root.viewBox.baseVal.width > 0) {
    root.setAttribute('width', String(root.viewBox.baseVal.width));
    root.setAttribute('height', String(root.viewBox.baseVal.height));
  }
  // Resolve CSS and currentColor while the browser has the engine styles.
  // Explicit pixel sizes also make MathJax's ex/em SVG units portable to resvg.
  const host = document.getElementById('output');
  host.replaceChildren(document.importNode(root, true));
  const mounted = host.firstElementChild;
  const box = mounted.getBoundingClientRect();
  const alignment = parseFloat(getComputedStyle(mounted).verticalAlign) || 0;
  if (box.width > 0 && box.height > 0) {
    mounted.setAttribute('width', String(box.width));
    mounted.setAttribute('height', String(box.height));
  }
  // Journey's SVG labels share the rectangle's classes; its default CSS can
  // therefore paint white text on pale tasks (and hide section headings).
  // Resolve label contrast against the actual adjacent rectangle before
  // freezing computed styles for the native SVG renderer.
  for (const label of mounted.querySelectorAll('text.task, text.journey-section')) {
    const rect = label.parentElement.querySelector('rect.task, rect.journey-section');
    if (!rect) continue;
    const channels = getComputedStyle(rect).fill.match(/[\d.]+/g);
    if (!channels || channels.length < 3) continue;
    const linear = channels.slice(0, 3).map(value => {
      const channel = Number(value) / 255;
      return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    });
    const luminance = linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
    const color = luminance > 0.179 ? '#000000' : '#ffffff';
    for (const text of [label, ...label.querySelectorAll('tspan')]) {
      text.style.setProperty('fill', color, 'important');
    }
  }
  for (const element of mounted.querySelectorAll('*')) {
    if (element.localName === 'script' || element.localName === 'foreignObject') {
      throw new Error('此图表使用了当前原生预览不支持的 HTML 标签');
    }
    for (const attr of [...element.attributes]) {
      if (attr.name.startsWith('on')) element.removeAttribute(attr.name);
      if (['href', 'xlink:href'].includes(attr.name) && !attr.value.startsWith('#')) {
        element.removeAttribute(attr.name);
      }
    }
    const style = getComputedStyle(element);
    for (const name of ['fill', 'stroke', 'font-family', 'font-size', 'font-weight', 'text-anchor']) {
      let value = style.getPropertyValue(name);
      // Browser CSSOM expands fragment paint URLs to absolute protocol URLs.
      value = value.replace(/url\(["']?[^#)]*#([^)'" ]+)["']?\)/g, 'url(#$1)');
      if (value) element.setAttribute(name, value);
    }
  }
  // Give dark native themes the same legible engine canvas as light themes.
  mounted.style.background = 'white';
  const background = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
  const viewBox = mounted.viewBox.baseVal;
  background.setAttribute('x', String(viewBox.x));
  background.setAttribute('y', String(viewBox.y));
  background.setAttribute('width', String(viewBox.width || box.width));
  background.setAttribute('height', String(viewBox.height || box.height));
  background.setAttribute('fill', 'white');
  mounted.insertBefore(background, mounted.firstChild);
  return {
    svg: new XMLSerializer().serializeToString(mounted),
    baseline: Math.max(0, Math.min(box.height, box.height + alignment)),
  };
}

export function runRenderer(render) {
async function main() {
  const requests = window.GLIM_REQUESTS;
  for (let index = 0; index < requests.length; index++) {
    let timeout;
    try {
      const result = normalize(await Promise.race([
        render(requests[index], index),
        new Promise((_, reject) => { timeout = setTimeout(() => reject(new Error(`此图表渲染超过 ${window.GLIM_LIMITS.DIAGRAM_BLOCK_SECONDS} 秒`)), window.GLIM_LIMITS.DIAGRAM_BLOCK_SECONDS * 1000); }),
      ]));
      if (new TextEncoder().encode(result.svg).byteLength > window.GLIM_LIMITS.DIAGRAM_SVG_BYTES) throw new Error(`图表 SVG 超过 ${window.GLIM_LIMITS.DIAGRAM_SVG_BYTES} 字节`) ;
      window.ipc.postMessage(JSON.stringify({ index, ...result }));
    } catch (error) {
      window.ipc.postMessage(JSON.stringify({ index, error: String(error).slice(0, 2000) }));
    } finally {
      clearTimeout(timeout);
    }
  }
  window.ipc.postMessage(JSON.stringify({ done: true }));
}
main().catch(error => window.ipc.postMessage(JSON.stringify({ fatal: String(error).slice(0, 2000) })));
}
