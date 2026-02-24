// Section loading and SVG interaction for Streaming Materialization paper

const sections = [
    { id: 'abstract', file: 'abstract.html' },
    { id: 'introduction', file: 'introduction.html' },
    { id: 'background', file: 'background.html' },
    { id: 'stream-chunk', file: 'stream-chunk.html' },
    { id: 'streaming-functions', file: 'streaming-functions.html' },
    { id: 'auto-wrapping', file: 'auto-wrapping.html' },
    { id: 'pipeline', file: 'pipeline.html' },
    { id: 'cas-invariants', file: 'cas-invariants.html' },
    { id: 'evaluation', file: 'evaluation.html' },
    { id: 'tradeoffs', file: 'tradeoffs.html' },
    { id: 'conclusion', file: 'conclusion.html' }
];

function initSvgHandlers(container) {
    // Batch vs Streaming toggle (introduction.html)
    const bvs = container.querySelector('#batch-vs-stream-svg');
    if (bvs) {
        let state = 0; // 0=both visible, 1=batch highlighted, 2=streaming highlighted
        const s0 = bvs.querySelector('#bvs-s0');
        const s1 = bvs.querySelector('#bvs-s1');
        const hint = bvs.querySelector('#bvs-hint');
        bvs.addEventListener('click', () => {
            state = (state + 1) % 3;
            if (state === 1) {
                s0.setAttribute('opacity', '1');
                s1.setAttribute('opacity', '0.15');
                hint.textContent = 'Batch: sequential stages, high memory, slow first byte — click again';
            } else if (state === 2) {
                s0.setAttribute('opacity', '0.15');
                s1.setAttribute('opacity', '1');
                hint.textContent = 'Streaming: concurrent stages, bounded memory, instant first byte — click again';
            } else {
                s0.setAttribute('opacity', '1');
                s1.setAttribute('opacity', '1');
                hint.textContent = '▶ click to highlight each mode';
            }
        });
    }

    // Background comparison scatter plot (background.html)
    const bgc = container.querySelector('#bg-compare-svg');
    if (bgc) {
        const info = {
            'bgc-flink': 'Flink: true streaming, external state backends, no content-addressing',
            'bgc-spark': 'Spark Structured Streaming: micro-batch, external storage (HDFS/S3/Delta)',
            'bgc-kafka': 'Kafka Streams: log-based streaming, RocksDB state, no computation-addressing',
            'bgc-unix': 'Unix pipes: kernel-buffered streaming, no caching, no DAG awareness',
            'bgc-bazel': 'Bazel: content-addressed remote cache, batch-only action execution',
            'bgc-nix': 'Nix: derivation-hash CAS, reproducible builds, strictly batch',
            'bgc-ipfs': 'IPFS: content-addressed block storage, no computation model',
            'bgc-dvc': 'DVC: Git-based data versioning, batch pipelines, external storage',
            'bgc-deriva': 'Deriva: streaming dataflow + content-addressed storage + DAG-aware caching'
        };
        const panel = bgc.querySelector('#bgc-panel');
        const items = bgc.querySelectorAll('.bgc-item, #bgc-deriva');
        items.forEach(g => {
            g.style.cursor = 'pointer';
            g.addEventListener('click', (e) => {
                e.stopPropagation();
                items.forEach(i => i.setAttribute('opacity', '0.3'));
                g.setAttribute('opacity', '1');
                panel.textContent = info[g.id] || '';
            });
        });
        bgc.addEventListener('click', () => {
            items.forEach(i => i.setAttribute('opacity', i.id === 'bgc-deriva' ? '0.9' : '0.8'));
            panel.textContent = '▶ click any system to see details';
        });
    }

    // Chunk lifecycle stepper (stream-chunk.html)
    const cl = container.querySelector('#chunk-lifecycle-svg');
    if (cl) {
        let phase = 0;
        const phases = [0,1,2,3];
        const dots = ['●','○'];
        const slotFills = [
            ['#2ecc71','#2ecc71','#2ecc71','#1a1e2e','#1a1e2e','#1a1e2e','#1a1e2e','#1a1e2e'],
            ['#f39c12','#f39c12','#f39c12','#f39c12','#f39c12','#f39c12','#f39c12','#f39c12'],
            ['#2ecc71','#2ecc71','#1a1e2e','#1a1e2e','#1a1e2e','#1a1e2e','#1a1e2e','#1a1e2e'],
            ['#e74c3c','#1a1e2e','#1a1e2e','#1a1e2e','#1a1e2e','#1a1e2e','#1a1e2e','#1a1e2e']
        ];
        const step = cl.querySelector('#cl-step');
        const hint = cl.querySelector('#cl-hint');
        function showPhase(p) {
            phases.forEach(i => {
                const g = cl.querySelector('#cl-phase-' + i);
                if (g) g.setAttribute('opacity', i === p ? '1' : '0');
            });
            for (let i = 0; i < 8; i++) {
                const s = cl.querySelector('#cl-slot-' + i);
                if (s) s.setAttribute('fill', slotFills[p][i]);
            }
            step.textContent = phases.map(i => i === p ? '●' : '○').join(' ');
            hint.textContent = 'step ' + (p+1) + '/4 — click to continue';
        }
        showPhase(0);
        cl.addEventListener('click', () => { phase = (phase + 1) % 4; showPhase(phase); });
    }

    // Function category explorer (streaming-functions.html)
    const fnc = container.querySelector('#fn-category-svg');
    if (fnc) {
        let active = 0;
        const cats = [1,2,3,4];
        const hint = fnc.querySelector('#fnc-hint');
        cats.forEach(c => {
            const g = fnc.querySelector('#fnc-cat-' + c);
            if (g) g.addEventListener('click', (e) => {
                e.stopPropagation();
                cats.forEach(i => {
                    const d = fnc.querySelector('#fnc-detail-' + i);
                    const b = fnc.querySelector('#fnc-cat-' + i + ' rect');
                    if (d) d.setAttribute('opacity', i === c && active !== c ? '1' : '0');
                    if (b) b.setAttribute('fill', i === c && active !== c ? '#2a2f4a' : '#232840');
                });
                active = active === c ? 0 : c;
                hint.textContent = active ? 'click same category to close, or another to switch' : '▶ click a category to see its functions';
            });
        });
    }

    // Auto-wrap toggle (auto-wrapping.html)
    const aw = container.querySelector('#auto-wrap-svg');
    if (aw) {
        let state = 0;
        const native = aw.querySelector('#aw-native');
        const batch = aw.querySelector('#aw-batch');
        const hint = aw.querySelector('#aw-hint');
        aw.addEventListener('click', () => {
            state = (state + 1) % 3;
            native.setAttribute('opacity', state === 2 ? '0.15' : '1');
            batch.setAttribute('opacity', state === 1 ? '0.15' : '1');
            hint.textContent = state === 1
                ? 'Streaming-native: bounded memory, instant first byte — click again'
                : state === 2
                ? 'Auto-wrapped: collect → batch → re-chunk, O(n) memory — click again'
                : '▶ click to highlight each path';
        });
    }

    // Pipeline build stepper (pipeline.html)
    const pb = container.querySelector('#pipeline-build-svg');
    if (pb) {
        let phase = 0;
        const total = 3;
        const step = pb.querySelector('#pb-step');
        const hint = pb.querySelector('#pb-hint');
        function showPB(p) {
            for (let i = 0; i < total; i++) {
                const g = pb.querySelector('#pb-phase-' + i);
                if (g) g.setAttribute('opacity', i === p ? '1' : '0');
            }
            step.textContent = Array.from({length: total}, (_, i) => i === p ? '●' : '○').join(' ');
            hint.textContent = 'step ' + (p+1) + '/' + total + ' — click to continue';
        }
        showPB(0);
        pb.addEventListener('click', () => { phase = (phase + 1) % total; showPB(phase); });
    }

    // CAS caching strategy toggle (cas-invariants.html)
    const cas = container.querySelector('#cas-cache-svg');
    if (cas) {
        let active = '';
        const opts = ['a','b','c'];
        const hint = cas.querySelector('#cas-hint');
        opts.forEach(o => {
            const g = cas.querySelector('#cas-opt-' + o);
            if (g) g.addEventListener('click', (e) => {
                e.stopPropagation();
                opts.forEach(i => {
                    const d = cas.querySelector('#cas-detail-' + i);
                    if (d) d.setAttribute('opacity', i === o && active !== o ? '1' : '0');
                });
                active = active === o ? '' : o;
                hint.textContent = active ? 'click same option to close, or another to switch' : '▶ click an option to see details';
            });
        });
    }

    /* ── sweep chart: hover/click data points, toggle legend ── */
    const sweep = container.querySelector('#sweep-chart-svg');
    if (sweep) {
        const tt = sweep.querySelector('#sweep-tooltip');
        const ttTitle = sweep.querySelector('#sweep-tt-title');
        const ttBatch = sweep.querySelector('#sweep-tt-batch');
        const ttStream = sweep.querySelector('#sweep-tt-stream');
        const ttGap = sweep.querySelector('#sweep-tt-gap');
        const ttWinner = sweep.querySelector('#sweep-tt-winner');
        const batchDots = sweep.querySelectorAll('#sweep-batch-dots circle');
        const allDots = sweep.querySelectorAll('#sweep-batch-dots circle, #sweep-stream-dots circle');

        allDots.forEach(dot => {
            dot.style.cursor = 'pointer';
            dot.addEventListener('click', () => {
                const cx = +dot.getAttribute('cx');
                // find the batch dot at same x for data
                const bd = [...batchDots].find(d => +d.getAttribute('cx') === cx) || dot;
                const wasShowing = tt.getAttribute('opacity') === '1' && ttTitle.textContent === bd.dataset.label;
                ttTitle.textContent = bd.dataset.label;
                ttBatch.textContent = '■ Batch: ' + bd.dataset.batch;
                ttStream.textContent = '■ Stream: ' + bd.dataset.stream;
                ttGap.textContent = '△ ' + bd.dataset.gap;
                ttWinner.textContent = '→ ' + bd.dataset.winner;
                const tx = Math.min(cx + 12, 650);
                const ty = Math.max(+dot.getAttribute('cy') - 50, 55);
                tt.setAttribute('transform', 'translate(' + tx + ',' + ty + ')');
                tt.setAttribute('opacity', wasShowing ? '0' : '1');
            });
        });

        // legend toggles
        const bLine = sweep.querySelector('#sweep-batch-line');
        const bArea = sweep.querySelector('#sweep-batch-area');
        const bDotsG = sweep.querySelector('#sweep-batch-dots');
        const sLine = sweep.querySelector('#sweep-stream-line');
        const sArea = sweep.querySelector('#sweep-stream-area');
        const sDotsG = sweep.querySelector('#sweep-stream-dots');

        sweep.querySelector('#sweep-legend-batch').addEventListener('click', e => {
            e.stopPropagation();
            const vis = bLine.getAttribute('opacity') === '0' ? '1' : (bLine.getAttribute('opacity') || '1') === '1' ? '0' : '1';
            [bLine, bArea, bDotsG].forEach(el => el.setAttribute('opacity', vis));
        });
        sweep.querySelector('#sweep-legend-stream').addEventListener('click', e => {
            e.stopPropagation();
            const vis = sLine.getAttribute('opacity') === '0' ? '1' : (sLine.getAttribute('opacity') || '1') === '1' ? '0' : '1';
            [sLine, sArea, sDotsG].forEach(el => el.setAttribute('opacity', vis));
        });

        // click background to dismiss tooltip
        sweep.addEventListener('click', e => {
            if (e.target === sweep || e.target.tagName === 'line' || e.target.tagName === 'rect') {
                tt.setAttribute('opacity', '0');
            }
        });
    }

    /* ── tradeoff radar: click boxes to show detail panels ── */
    const tdr = container.querySelector('#tradeoff-radar-svg');
    if (tdr) {
        const keys = ['throughput','memory','chunk','autowrap','accumulator'];
        let active = '';
        keys.forEach(k => {
            tdr.querySelector('#td-' + k).addEventListener('click', () => {
                keys.forEach(j => {
                    tdr.querySelector('#td-detail-' + j).setAttribute('opacity', j === k && active !== k ? '1' : '0');
                    tdr.querySelector('#td-' + j + ' rect').setAttribute('fill', j === k && active !== k ? '#2a3050' : '#232840');
                });
                active = active === k ? '' : k;
            });
        });
    }

    /* ── mode selection: click nodes to show result ── */
    const ms = container.querySelector('#mode-select-svg');
    if (ms) {
        const keys = ['cached','streaming','batch'];
        keys.forEach(k => {
            ms.querySelector('#ms-' + k).addEventListener('click', () => {
                const r = ms.querySelector('#ms-result-' + k);
                const vis = r.getAttribute('opacity') === '1' ? '0' : '1';
                r.setAttribute('opacity', vis);
            });
        });
    }
}

function attachCodeBlockButtons(container) {
    container.querySelectorAll('.code-block').forEach(block => {
        if (block.querySelector('.copy-btn')) return;
        const btn = document.createElement('button');
        btn.className = 'copy-btn';
        btn.textContent = 'Copy';
        btn.addEventListener('click', () => {
            const code = block.querySelector('code') || block;
            const text = code.innerText;
            navigator.clipboard.writeText(text).then(() => {
                btn.textContent = 'Copied!';
                setTimeout(() => btn.textContent = 'Copy', 2000);
            });
        });
        block.style.position = 'relative';
        block.appendChild(btn);
    });
}

sections.forEach(section => {
    const container = document.getElementById(section.id + '-container');
    if (!container) return;

    fetch(section.file)
        .then(r => {
            if (!r.ok) throw new Error('Not found: ' + section.file);
            return r.text();
        })
        .then(html => {
            const doc = new DOMParser().parseFromString(html, 'text/html');
            container.innerHTML = doc.body.innerHTML;
            attachCodeBlockButtons(container);
            initSvgHandlers(container);
            container.querySelectorAll('a[href^="#"]').forEach(a => {
                a.addEventListener('click', function(e) {
                    e.preventDefault();
                    const t = document.querySelector(this.getAttribute('href'));
                    if (t) t.scrollIntoView({ behavior: 'smooth', block: 'start' });
                });
            });
        })
        .catch(() => {
            container.innerHTML = '<p style="color:#e74c3c;">Failed to load section.</p>';
        });
});

// Smooth scrolling for anchor links
document.querySelectorAll('a[href^="#"]').forEach(anchor => {
    anchor.addEventListener('click', function(e) {
        e.preventDefault();
        const target = document.querySelector(this.getAttribute('href'));
        if (target) {
            target.scrollIntoView({ behavior: 'smooth', block: 'start' });
        }
    });
});
