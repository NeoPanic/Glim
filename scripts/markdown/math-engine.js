import { mathjax } from 'mathjax-full/js/mathjax.js';
import { TeX } from 'mathjax-full/js/input/tex.js';
import { SVG } from 'mathjax-full/js/output/svg.js';
import { liteAdaptor } from 'mathjax-full/js/adaptors/liteAdaptor.js';
import { RegisterHTMLHandler } from 'mathjax-full/js/handlers/html.js';
import { AllPackages } from 'mathjax-full/js/input/tex/AllPackages.js';

const adaptor = liteAdaptor();
RegisterHTMLHandler(adaptor);
const mathDocument = mathjax.document('', {
  InputJax: new TeX({
    // noundefined paints unknown commands red instead of reporting a parse
    // error, which would make malformed or disabled commands look successful.
    packages: AllPackages.filter(name => !['require', 'autoload', 'html', 'noundefined'].includes(name)),
    maxBuffer: window.GLIM_LIMITS.DIAGRAM_SOURCE_BYTES,
    maxMacros: window.GLIM_LIMITS.DIAGRAM_MATH_MACROS,
  }),
  OutputJax: new SVG({ fontCache: 'none' }),
});

export function renderMath({ kind, source }) {
  const host = document.getElementById('output');
      const node = mathDocument.convert(source, {
        display: kind !== 'inline-math', em: 16, ex: 8, containerWidth: 1000,
      });
      const markup = adaptor.outerHTML(node);
      host.innerHTML = markup;
      const error = host.querySelector('[data-mjx-error]');
      if (error) throw new Error(`数学公式语法无法识别：${error.getAttribute('data-mjx-error') || '解析错误'}`);
      const svg = host.querySelector('svg');
      if (!svg) throw new Error('数学公式没有返回 SVG');
      return svg.outerHTML;
}
