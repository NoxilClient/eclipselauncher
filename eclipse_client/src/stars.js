'use strict';
// Twinkling stars + shooting stars with trails, drawn over the background image.
(function () {
  const cv = document.getElementById('fx'), ctx = cv.getContext('2d');
  const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  let W = 0, H = 0, dpr = 1, stars = [], shooters = [], nextShot = 1.5, shootOn = true, last = performance.now();

  function resize() {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    W = innerWidth; H = innerHeight;
    cv.width = W * dpr; cv.height = H * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const n = Math.round((W * H) / 5500);
    stars = Array.from({ length: n }, () => ({
      x: Math.random() * W, y: Math.random() * H,
      r: 0.4 + Math.random() * 1.1,
      base: 0.25 + Math.random() * 0.5,
      speed: 0.6 + Math.random() * 2.2,
      phase: Math.random() * Math.PI * 2,
      big: Math.random() < 0.12            // some stars flare up brighter
    }));
  }

  function drawStars(t) {
    for (const s of stars) {
      const k = 0.5 + 0.5 * Math.sin(t * s.speed + s.phase);   // 0..1
      const a = reduce ? s.base : s.base * (0.25 + 0.75 * k) + (s.big ? 0.45 * Math.pow(k, 6) : 0);
      ctx.globalAlpha = Math.min(a, 1);
      ctx.fillStyle = '#fff';
      ctx.beginPath(); ctx.arc(s.x, s.y, s.r, 0, 6.2832); ctx.fill();
      if (s.big && !reduce && k > 0.8) {                         // soft glow when lit
        const g = ctx.createRadialGradient(s.x, s.y, 0, s.x, s.y, s.r * 7);
        g.addColorStop(0, 'rgba(255,255,255,.35)'); g.addColorStop(1, 'rgba(255,255,255,0)');
        ctx.globalAlpha = (k - 0.8) * 5; ctx.fillStyle = g;
        ctx.beginPath(); ctx.arc(s.x, s.y, s.r * 7, 0, 6.2832); ctx.fill();
      }
    }
  }

  function spawn() {
    const fromLeft = Math.random() < 0.5;
    const ang = (20 + Math.random() * 25) * Math.PI / 180;       // downward angle
    const dir = fromLeft ? 1 : -1, v = 650 + Math.random() * 500;
    shooters.push({
      x: fromLeft ? Math.random() * W * 0.6 : W * 0.4 + Math.random() * W * 0.6,
      y: Math.random() * H * 0.45 - 20,
      vx: Math.cos(ang) * v * dir, vy: Math.sin(ang) * v,
      len: 140 + Math.random() * 140, life: 0, max: 1.1 + Math.random() * 0.7
    });
  }

  function drawShooters(dt) {
    for (let i = shooters.length - 1; i >= 0; i--) {
      const s = shooters[i];
      s.x += s.vx * dt; s.y += s.vy * dt; s.life += dt;
      const fade = Math.max(0, 1 - s.life / s.max);
      const sp = Math.hypot(s.vx, s.vy), tx = s.x - (s.vx / sp) * s.len, ty = s.y - (s.vy / sp) * s.len;
      const g = ctx.createLinearGradient(s.x, s.y, tx, ty);
      g.addColorStop(0, 'rgba(255,255,255,' + fade + ')');
      g.addColorStop(0.3, 'rgba(200,215,255,' + fade * 0.45 + ')');
      g.addColorStop(1, 'rgba(255,255,255,0)');
      ctx.globalAlpha = 1; ctx.strokeStyle = g; ctx.lineWidth = 1.8; ctx.lineCap = 'round';
      ctx.beginPath(); ctx.moveTo(s.x, s.y); ctx.lineTo(tx, ty); ctx.stroke();
      const h = ctx.createRadialGradient(s.x, s.y, 0, s.x, s.y, 7);   // glowing head
      h.addColorStop(0, 'rgba(255,255,255,' + fade + ')'); h.addColorStop(1, 'rgba(255,255,255,0)');
      ctx.fillStyle = h; ctx.beginPath(); ctx.arc(s.x, s.y, 7, 0, 6.2832); ctx.fill();
      if (s.life > s.max || s.x < -300 || s.x > W + 300 || s.y > H + 300) shooters.splice(i, 1);
    }
  }

  function frame(now) {
    const dt = Math.min((now - last) / 1000, 0.05); last = now;
    ctx.clearRect(0, 0, W, H);
    drawStars(now / 1000);
    if (shootOn && !reduce) {
      nextShot -= dt;
      if (nextShot <= 0) { spawn(); nextShot = 1.8 + Math.random() * 3.5; }
      drawShooters(dt);
    }
    requestAnimationFrame(frame);
  }

  addEventListener('resize', resize);
  resize(); requestAnimationFrame(frame);
  window.ecFx = { setShooting(v) { shootOn = !!v; if (!v) shooters = []; } };
})();
