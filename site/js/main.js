// Boot: the page chrome (nav, mock menu bar, tray menu, music window), then the bear.

import * as S from './sprites.js';
import { HeroBear } from './hero.js';
import { initSections } from './sections.js';

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

function phaseOf(h) {
  if (h >= 5 && h < 8) return 'dawn';
  if (h >= 8 && h < 17) return 'day';
  if (h >= 17 && h < 20) return 'dusk';
  return 'night';
}

function initClock() {
  const el = $('[data-clock]');
  const desk = $('#desk');
  const fmt = new Intl.DateTimeFormat(undefined, { weekday: 'short', hour: 'numeric', minute: '2-digit' });
  const tick = () => {
    const now = new Date();
    if (el) el.textContent = fmt.format(now);
    if (desk) desk.dataset.time = phaseOf(now.getHours());
  };
  tick();
  setInterval(tick, 15000);
  for (const y of $$('[data-year]')) y.textContent = String(new Date().getFullYear());
}

function initNav() {
  const nav = $('#nav');
  const top = $('.hero');
  if (!nav || !top) return;
  new IntersectionObserver(([e]) => nav.classList.toggle('is-stuck', !e.isIntersecting), { rootMargin: '-80px 0px 0px 0px' })
    .observe($('.hero-copy') || top);
}

function initTray(hero) {
  const btn = $('.tray-btn');
  const menu = $('#tray-menu');
  if (!btn || !menu) return;
  const open = (v) => {
    menu.hidden = !v;
    btn.setAttribute('aria-expanded', String(v));
    if (v) $('button', menu).focus({ preventScroll: true });
  };
  btn.addEventListener('click', (e) => {
    e.stopPropagation();
    open(menu.hidden);
  });
  document.addEventListener('click', (e) => {
    if (!menu.hidden && !menu.contains(e.target)) open(false);
  });
  menu.addEventListener('keydown', (e) => {
    const items = $$('button', menu);
    const i = items.indexOf(document.activeElement);
    if (e.key === 'Escape') { open(false); btn.focus(); }
    if (e.key === 'ArrowDown') { e.preventDefault(); items[(i + 1) % items.length].focus(); }
    if (e.key === 'ArrowUp') { e.preventDefault(); items[(i - 1 + items.length) % items.length].focus(); }
  });
  const go = (sel, focus) => {
    open(false);
    const el = $(sel);
    if (!el) return;
    el.scrollIntoView({ behavior: S.reduceMotion.matches ? 'auto' : 'smooth', block: 'start' });
    if (focus) setTimeout(() => $(focus)?.focus({ preventScroll: true }), 500);
  };
  menu.addEventListener('click', (e) => {
    const b = e.target.closest('button');
    if (!b) return;
    const act = b.dataset.tm;
    if (b.getAttribute('role') === 'menuitemcheckbox') {
      const on = b.getAttribute('aria-checked') !== 'true';
      b.setAttribute('aria-checked', String(on));
      if (act === 'dnd' && hero) hero.say(on ? 'Shh. I’ll keep quiet.' : 'I’m back!', { hold: 2000 });
      if (act === 'fullscreen' && hero) hero.say(on ? 'I’ll duck out over fullscreen apps.' : 'I’ll stay where you can see me.', { hold: 2400 });
      if (act === 'quiet' && hero) hero.say(on ? 'Quiet hours on. Sleep tight.' : 'Quiet hours off.', { hold: 2000 });
      if (act === 'scenes') {
        const sw = $('[data-scenes]');
        if (sw) sw.checked = on;
      }
      return;
    }
    if (act === 'ask') go('#talk', '#ask-input');
    else if (act === 'approvals') go('#talk', '.modes input:checked');
    else if (act === 'model') { go('#faq-ai'); const d = $('#faq-ai'); if (d) d.open = true; }
    else if (act === 'floor') go('#rooms');
    else if (act === 'studio') go('#studio');
    else if (act === 'hide' && hero) {
      const h = !hero.hidden;
      hero.setHidden(h);
      b.textContent = h ? 'Show bear' : 'Hide bear';
      open(false);
    } else if (act === 'quit' && hero) {
      open(false);
      hero.say('Nice try. I live here now.', { hold: 2400 });
    }
  });
}

function initMusic(hero) {
  const np = $('.win-np');
  const play = $('.np-play');
  if (!np || !play) return;
  play.addEventListener('click', (e) => {
    e.stopPropagation();
    const on = play.getAttribute('aria-pressed') !== 'true';
    play.setAttribute('aria-pressed', String(on));
    play.setAttribute('aria-label', on ? 'Pause music' : 'Play music');
    np.classList.toggle('is-playing', on);
    if (hero) hero.music(on);
  });
}

async function boot() {
  initClock();
  initNav();
  let hero = null;
  try {
    await S.load(new URL('../assets/bear.json', import.meta.url).href);
  } catch (e) {
    console.error('2poke: could not load the bear', e);
    document.documentElement.classList.add('no-bear');
    initSections(null);
    return;
  }
  const desk = $('#desk');
  if (desk) {
    hero = new HeroBear(desk);
    hero.start().catch((e) => console.error('2poke hero', e));
    $('[data-poke]')?.addEventListener('click', () => hero.poke());
  }
  initTray(hero);
  initMusic(hero);
  initSections(hero);
}

boot();
