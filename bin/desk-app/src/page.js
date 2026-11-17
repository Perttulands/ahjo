// The Desk's layer over a page an agent published: in Comment mode a click
// marks the element under it (its selector, its words and where it is on the
// page) and tells the app; the app's marks are drawn back over the page as
// numbered badges. Nothing on the page changes while browsing.
(() => {
  if (window.__desk) return;
  const INK = '#d7b879';
  const post = m => {
    try { window.ipc.postMessage(JSON.stringify(m)); } catch (e) { /* not in the app */ }
  };
  const unit = v => Math.min(1, Math.max(0, v));
  let mode = 'browse', marks = [], selected = null, layer = null, hover = null;

  // A selector that finds this element again: a unique id, else the path of
  // tags (with their place among same-named siblings) up to the body.
  function selector(el) {
    const parts = [];
    for (let e = el; e && e.nodeType === 1 && e !== document.documentElement; e = e.parentElement) {
      if (e.id && document.querySelectorAll('#' + CSS.escape(e.id)).length === 1) {
        parts.unshift('#' + CSS.escape(e.id));
        break;
      }
      let part = e.localName;
      const same = e.parentElement ? [...e.parentElement.children].filter(c => c.localName === e.localName) : [];
      if (same.length > 1) part += `:nth-of-type(${same.indexOf(e) + 1})`;
      parts.unshift(part);
      if (e.localName === 'body') break;
    }
    return parts.join(' > ');
  }
  const words = el => (el.innerText || el.getAttribute('alt') || el.getAttribute('aria-label') || el.getAttribute('title') || '')
    .replace(/\s+/g, ' ').trim().slice(0, 300);
  const find = sel => { try { return sel ? document.querySelector(sel) : null; } catch (e) { return null; } };

  function box(color, width) {
    const b = document.createElement('div');
    Object.assign(b.style, {
      position: 'absolute', pointerEvents: 'none', boxSizing: 'border-box',
      border: `${width}px solid ${color}`, borderRadius: '3px', boxShadow: '0 0 0 1px rgba(0,0,0,.6)',
    });
    return b;
  }
  function place(b, el) {
    const r = el.getBoundingClientRect();
    Object.assign(b.style, { left: `${r.left + scrollX}px`, top: `${r.top + scrollY}px`, width: `${r.width}px`, height: `${r.height}px` });
  }
  function ensureLayer() {
    if (layer && layer.isConnected) return layer;
    layer = document.createElement('div');
    layer.setAttribute('data-desk', '');
    Object.assign(layer.style, { position: 'absolute', left: '0', top: '0', width: '0', height: '0', zIndex: '2147483647', pointerEvents: 'none' });
    document.documentElement.appendChild(layer);
    return layer;
  }

  function draw() {
    const l = ensureLayer();
    l.replaceChildren();
    const d = document.documentElement, w = d.scrollWidth, h = d.scrollHeight;
    for (const m of marks) {
      const el = find(m.selector), color = m.color || INK, on = m.id === selected;
      if (el && on) { const b = box(color, 3); place(b, el); l.appendChild(b); }
      const badge = document.createElement('div');
      badge.textContent = String(m.n);
      badge.setAttribute('data-desk-mark', m.id);
      const r = el ? el.getBoundingClientRect() : null;
      const x = r ? r.left + scrollX : m.x * w, y = r ? r.top + scrollY : m.y * h;
      Object.assign(badge.style, {
        position: 'absolute', left: `${Math.max(0, x - 10)}px`, top: `${Math.max(0, y - 10)}px`,
        minWidth: '20px', height: '20px', padding: '0 5px', boxSizing: 'border-box', borderRadius: '10px',
        background: color, color: '#111', font: 'bold 12px/20px system-ui, sans-serif', textAlign: 'center',
        boxShadow: on ? '0 0 0 3px rgba(0,0,0,.75)' : '0 0 0 1px rgba(0,0,0,.6)',
      });
      l.appendChild(badge);
    }
    if (hover) l.appendChild(hover);
  }

  const swallow = e => { if (mode === 'comment') { e.preventDefault(); e.stopPropagation(); } };
  for (const t of ['mousedown', 'mouseup', 'pointerdown', 'pointerup', 'dblclick', 'submit']) document.addEventListener(t, swallow, true);
  document.addEventListener('mousemove', e => {
    if (mode !== 'comment' || !(e.target instanceof Element)) return;
    hover = hover || box(INK, 2);
    place(hover, e.target);
    ensureLayer().appendChild(hover);
  }, true);
  document.addEventListener('click', e => {
    if (mode !== 'comment' || !(e.target instanceof Element)) return;
    e.preventDefault();
    e.stopPropagation();
    const d = document.documentElement;
    post({
      kind: 'mark', selector: selector(e.target), quote: words(e.target),
      x: unit(e.pageX / Math.max(1, d.scrollWidth)), y: unit(e.pageY / Math.max(1, d.scrollHeight)),
    });
  }, true);
  addEventListener('resize', () => draw());

  window.__desk = {
    mode(m) {
      mode = m === 'comment' ? 'comment' : 'browse';
      document.documentElement.style.cursor = mode === 'comment' ? 'crosshair' : '';
      if (mode !== 'comment' && hover) { hover.remove(); hover = null; }
    },
    show(list, on) {
      marks = list || [];
      selected = on || null;
      draw();
      const m = marks.find(m => m.id === selected), el = m && find(m.selector);
      if (el) el.scrollIntoView({ block: 'nearest' });
    },
  };
  const ready = () => post({ kind: 'ready' });
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', ready); else ready();
})();
