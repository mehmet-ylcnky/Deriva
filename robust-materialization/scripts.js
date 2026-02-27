document.addEventListener('DOMContentLoaded', function() {
    // Rust syntax highlighting
    function highlightRust(code) {
        const keywords = ['fn', 'pub', 'struct', 'impl', 'let', 'mut', 'async', 'await', 'use', 'mod', 'crate', 'self', 'Self', 'return', 'if', 'else', 'match', 'for', 'while', 'loop', 'break', 'continue', 'const', 'static', 'trait', 'enum', 'type', 'where', 'unsafe', 'extern', 'move', 'ref', 'dyn'];
        const builtins = ['Box', 'Arc', 'Vec', 'Option', 'Result', 'Some', 'None', 'Ok', 'Err', 'Send', 'Sync', 'Clone', 'Debug', 'Default', 'Drop', 'From', 'Into', 'Iterator', 'Future', 'Pin'];
        const types = ['String', 'str', 'u8', 'u16', 'u32', 'u64', 'i8', 'i16', 'i32', 'i64', 'f32', 'f64', 'bool', 'usize', 'isize', 'CAddr', 'Recipe', 'Bytes', 'HashMap', 'BTreeMap', 'HashSet', 'Mutex', 'RwLock', 'Db', 'Tree', 'PersistentDag', 'AsyncExecutor', 'VerificationMode', 'DerivaError', 'ComputeFunction', 'FunctionId', 'Value', 'ComputeCost', 'GcConfig', 'GcResult', 'CascadePolicy', 'CascadeResult', 'CascadeInvalidator', 'DagStore', 'StorageBackend', 'ServerState', 'BlobStore', 'EvictableCache', 'MaterializationCache', 'FunctionRegistry', 'StreamChunk', 'StreamPipeline', 'Semaphore', 'Receiver', 'Sender', 'JoinHandle', 'Histogram', 'Counter', 'IntCounter', 'IntGauge'];

        // Escape HTML first
        code = code.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

        // Highlight comments (must be first to avoid highlighting inside comments)
        code = code.replace(/(\/\/.*$)/gm, '<span class="rust-comment">$1</span>');

        // Highlight strings
        code = code.replace(/("(?:[^"\\]|\\.)*")/g, '<span class="rust-string">$1</span>');

        // Highlight macros (e.g., println!, assert_eq!, vec!, lazy_static!)
        code = code.replace(/\b([a-z_][a-z0-9_]*!)\s*/g, '<span class="rust-function">$1</span> ');

        // Highlight keywords
        keywords.forEach(kw => {
            code = code.replace(new RegExp('\\b(' + kw + ')\\b', 'g'), '<span class="rust-keyword">$1</span>');
        });

        // Highlight built-in traits/types
        builtins.forEach(b => {
            code = code.replace(new RegExp('\\b(' + b + ')\\b', 'g'), '<span class="rust-type">$1</span>');
        });

        // Highlight custom types
        types.forEach(t => {
            code = code.replace(new RegExp('\\b(' + t + ')\\b', 'g'), '<span class="rust-type">$1</span>');
        });

        // Highlight function definitions (fn name)
        code = code.replace(/\b(fn)\b(\s+)([a-z_][a-z0-9_]*)/g, '<span class="rust-keyword">$1</span>$2<span class="rust-function">$3</span>');

        // Highlight lifetime annotations
        code = code.replace(/('(?:[a-z_]+))/g, '<span class="rust-keyword">$1</span>');

        // Highlight numeric literals
        code = code.replace(/\b(\d+(?:\.\d+)?(?:_\d+)*(?:u8|u16|u32|u64|i8|i16|i32|i64|f32|f64|usize|isize)?)\b/g, '<span class="rust-string">$1</span>');

        return code;
    }

    // Apply highlighting + buttons to all code blocks in a container
    function enhanceCodeBlocks(container) {
        container.querySelectorAll('.code-block pre').forEach(pre => {
            const code = pre.textContent;
            // Highlight if it looks like Rust code
            if (/\b(fn |pub |struct |impl |let |async |use |mod |trait |enum )/.test(code)) {
                pre.innerHTML = highlightRust(code);
            }
        });

        container.querySelectorAll('.code-block').forEach(block => {
            if (block.querySelector('.copy-btn')) return; // already enhanced

            var cb = document.createElement('button');
            cb.textContent = 'Copy'; cb.className = 'copy-btn';
            var tb = document.createElement('button');
            tb.textContent = '🌙'; tb.className = 'theme-btn'; tb.title = 'Toggle theme';
            block.appendChild(cb); block.appendChild(tb);

            cb.addEventListener('click', function() {
                var t = block.querySelector('pre').textContent;
                navigator.clipboard.writeText(t).then(function() {
                    cb.textContent = 'Copied!';
                    setTimeout(function() { cb.textContent = 'Copy'; }, 2000);
                });
            });
            tb.addEventListener('click', function() {
                block.classList.toggle('dark-theme');
                tb.textContent = block.classList.contains('dark-theme') ? '☀️' : '🌙';
            });
        });
    }

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

    // Collapsible ToC
    document.querySelectorAll('.toc-toggle').forEach(toggle => {
        toggle.addEventListener('click', function() {
            this.classList.toggle('expanded');
            const subList = this.parentElement.querySelector('.toc-sub');
            if (subList) subList.classList.toggle('collapsed');
        });
    });

    // Interactive DAG SVG toggle
    function initDagToggle(container) {
        // PersistentDag forward/reverse toggle
        var svg = container.querySelector('#dag-svg');
        if (svg) {
            var fwd = svg.querySelector('#fwd-edges');
            var rev = svg.querySelector('#rev-edges');
            var fwdEx = svg.querySelector('#fwd-explain');
            var revEx = svg.querySelector('#rev-explain');
            var hint = svg.querySelector('#dag-hint');
            var showing = 'fwd';
            svg.addEventListener('click', function() {
                var t = 'opacity 0.4s';
                fwd.style.transition = t; rev.style.transition = t;
                fwdEx.style.transition = t; revEx.style.transition = t;
                if (showing === 'fwd') {
                    fwd.style.opacity = '0'; rev.style.opacity = '1';
                    fwdEx.style.opacity = '0'; revEx.style.opacity = '1';
                    hint.textContent = '\u2190 Click to toggle forward edges';
                    showing = 'rev';
                } else {
                    fwd.style.opacity = '1'; rev.style.opacity = '0';
                    fwdEx.style.opacity = '1'; revEx.style.opacity = '0';
                    hint.textContent = 'Click to toggle reverse edges \u2192';
                    showing = 'fwd';
                }
            });
        }

        // Semaphore deadlock toggle
        var semsvg = container.querySelector('#sem-svg');
        if (semsvg) {
            var semstate = 0;
            semsvg.addEventListener('click', function() {
                var tr = 'opacity 0.4s';
                semstate = 1 - semstate;
                var s0 = semsvg.querySelector('#sem-s0');
                var s1 = semsvg.querySelector('#sem-s1');
                s0.style.transition = tr; s1.style.transition = tr;
                s0.style.opacity = semstate === 0 ? '1' : '0';
                s1.style.opacity = semstate === 1 ? '1' : '0';
                var sh = semsvg.querySelector('#sem-hint');
                if (sh) sh.textContent = semstate === 0
                    ? 'Click to see correct ordering \u2192'
                    : '\u2190 Click to see deadlock scenario';
            });
        }

        // Parallel input resolution toggle
        var ressvg = container.querySelector('#resolve-svg');
        if (ressvg) {
            var resstate = 0;
            ressvg.addEventListener('click', function() {
                var tr = 'opacity 0.4s';
                resstate = 1 - resstate;
                var s0 = ressvg.querySelector('#res-s0');
                var s1 = ressvg.querySelector('#res-s1');
                s0.style.transition = tr; s1.style.transition = tr;
                s0.style.opacity = resstate === 0 ? '1' : '0';
                s1.style.opacity = resstate === 1 ? '1' : '0';
                var rh = ressvg.querySelector('#res-hint');
                if (rh) rh.textContent = resstate === 0
                    ? 'Click to see parallel resolution \u2192'
                    : '\u2190 Click to see sequential resolution';
            });
        }

        // Deterministic ordering toggle
        var ordsvg = container.querySelector('#order-svg');
        if (ordsvg) {
            var ordstate = 0;
            ordsvg.addEventListener('click', function() {
                var tr = 'opacity 0.4s';
                ordstate = 1 - ordstate;
                var s0 = ordsvg.querySelector('#ord-s0');
                var s1 = ordsvg.querySelector('#ord-s1');
                s0.style.transition = tr; s1.style.transition = tr;
                s0.style.opacity = ordstate === 0 ? '1' : '0';
                s1.style.opacity = ordstate === 1 ? '1' : '0';
                var oh = ordsvg.querySelector('#ord-hint');
                if (oh) oh.textContent = ordstate === 0
                    ? 'Click to see reversed completion order \u2192'
                    : '\u2190 Click to see normal completion order';
            });
        }

        // Executor comparison toggle
        var cmpsvg = container.querySelector('#cmp-svg');
        if (cmpsvg) {
            var cmpstate = 0;
            cmpsvg.addEventListener('click', function() {
                var tr = 'opacity 0.4s';
                cmpstate = 1 - cmpstate;
                var s0 = cmpsvg.querySelector('#cmp-s0');
                var s1 = cmpsvg.querySelector('#cmp-s1');
                s0.style.transition = tr; s1.style.transition = tr;
                s0.style.opacity = cmpstate === 0 ? '1' : '0';
                s1.style.opacity = cmpstate === 1 ? '1' : '0';
                var ch = cmpsvg.querySelector('#cmp-hint');
                if (ch) ch.textContent = cmpstate === 0
                    ? 'Click to see AsyncExecutor \u2192'
                    : '\u2190 Click to see original executor';
            });
        }

        // Dedup timeline step-through
        var ddsvg = container.querySelector('#dedup-svg');
        if (ddsvg) {
            var ddstep = 0;
            var ddmax = 3;
            var ddcolors = ['#81d4fa', '#f39c12', '#f39c12', '#2ecc71'];
            ddsvg.addEventListener('click', function() {
                var tr = 'opacity 0.35s';
                ddstep = (ddstep + 1) % (ddmax + 1);
                for (var i = 0; i <= ddmax; i++) {
                    var g = ddsvg.querySelector('#dd-s' + i);
                    var d = ddsvg.querySelector('#dd-d' + i);
                    if (g) { g.style.transition = tr; g.style.opacity = i === ddstep ? '1' : '0'; }
                    if (d) d.setAttribute('fill', i === ddstep ? ddcolors[i] : '#3a3f5c');
                }
                var dh = ddsvg.querySelector('#dd-hint');
                if (dh) dh.textContent = ddstep === ddmax ? 'Click to restart \u2192' : 'Click to step through timeline \u2192';
            });
        }

        // Pipeline step-through (11 steps)
        var pipesvg = container.querySelector('#pipe-svg');
        if (pipesvg) {
            var pstep = 0;
            var pmax = 10;
            var boxColors = ['#2ecc71','#2ecc71','#f39c12','#e74c3c','#e74c3c','#e74c3c','#e74c3c','#e74c3c','#e74c3c','#e74c3c','#e74c3c'];
            pipesvg.addEventListener('click', function() {
                var tr = 'opacity 0.3s';
                pstep = (pstep + 1) % (pmax + 1);
                for (var i = 0; i <= pmax; i++) {
                    var g = pipesvg.querySelector('#pipe-s' + i);
                    var box = pipesvg.querySelector('#pipe-box-' + i);
                    if (g) { g.style.transition = tr; g.style.opacity = i === pstep ? '1' : '0'; }
                    if (box) {
                        box.setAttribute('stroke', i === pstep ? boxColors[i] : '#3a3f5c');
                        box.setAttribute('stroke-width', i === pstep ? '2.5' : '1');
                        box.setAttribute('fill', i === pstep ? '#2a2f48' : '#232840');
                    }
                }
                var cnt = pipesvg.querySelector('#pipe-counter');
                if (cnt) cnt.textContent = 'Step ' + (pstep + 1) + ' of 11';
                var ph = pipesvg.querySelector('#pipe-hint');
                if (ph) ph.textContent = pstep === pmax ? 'Click to restart \u2192' : 'Click to advance \u2192';
            });
        }

        // Restart recovery toggle
        var rsvg = container.querySelector('#restart-svg');
        if (rsvg) {
            var rstate = 0;
            var rhint = container.querySelector('#rst-hint');
            rsvg.addEventListener('click', function() {
                var tr = 'opacity 0.4s';
                rstate = 1 - rstate;
                var s0 = rsvg.querySelector('#rst-s0');
                var s1 = rsvg.querySelector('#rst-s1');
                s0.style.transition = tr; s1.style.transition = tr;
                s0.style.opacity = rstate === 0 ? '1' : '0';
                s1.style.opacity = rstate === 1 ? '1' : '0';
                if (rhint) rhint.textContent = rstate === 0
                    ? 'Click to compare: persistent DAG \u2192'
                    : '\u2190 Click to compare: in-memory DagStore';
            });
            if (rhint) rhint.addEventListener('click', function() { rsvg.dispatchEvent(new Event('click')); });
        }

        // Query operations step-through
        var qsvg = container.querySelector('#query-svg');
        if (qsvg) {
            var qstep = 0;
            var qmax = 2;
            var qcolors = ['#81d4fa', '#ffb74d', '#a5d6a7'];
            var qhints = ['Click to cycle through query operations \u2192', 'dependents(A) \u2014 reverse lookup \u2192', 'live_addr_set() \u2014 full scan \u2192'];
            qsvg.addEventListener('click', function() {
                var tr = 'opacity 0.35s';
                qstep = (qstep + 1) % (qmax + 1);
                for (var i = 0; i <= qmax; i++) {
                    var g = qsvg.querySelector('#qry-s' + i);
                    var d = qsvg.querySelector('#qry-d' + i);
                    if (g) { g.style.transition = tr; g.style.opacity = i === qstep ? '1' : '0'; }
                    if (d) { d.setAttribute('fill', i === qstep ? qcolors[i] : '#3a3f5c'); }
                }
                var qh = qsvg.querySelector('#qry-hint');
                if (qh) qh.textContent = qhints[qstep];
            });
        }

        // Transactional insert step-through
        var tsvg = container.querySelector('#txn-svg');
        if (tsvg) {
            var tstep = 0;
            var tmax = 3;
            tsvg.addEventListener('click', function() {
                var tr = 'opacity 0.35s';
                tstep = (tstep + 1) % (tmax + 1);
                for (var i = 0; i <= tmax; i++) {
                    var g = tsvg.querySelector('#txn-s' + i);
                    var e = tsvg.querySelector('#txn-e' + i);
                    var d = tsvg.querySelector('#txn-d' + i);
                    if (g) { g.style.transition = tr; g.style.opacity = i === tstep ? '1' : '0'; }
                    if (e) { e.style.transition = tr; e.style.opacity = i === tstep ? '1' : '0'; }
                    if (d) { d.setAttribute('fill', i === tstep ? '#81d4fa' : '#3a3f5c'); }
                }
                var th = tsvg.querySelector('#txn-hint');
                if (th) {
                    var labels = ['Click to step through the transaction \u2192', 'Step 1/3: Forward edge \u2192', 'Step 2/3: Reverse edge \u2192', 'Crash scenario \u2014 click to restart \u2192'];
                    th.textContent = labels[tstep];
                }
            });
        }

        // Non-determinism step-through
        var ndsvg = container.querySelector('#nondet-svg');
        if (ndsvg) {
            var ndstep = 0;
            var ndmax = 3;
            var ndcolors = ['#81d4fa', '#f39c12', '#e74c3c', '#e74c3c'];
            ndsvg.addEventListener('click', function() {
                ndstep = (ndstep + 1) % (ndmax + 1);
                for (var i = 0; i <= ndmax; i++) {
                    var g = ndsvg.querySelector('#nd-s' + i);
                    var d = ndsvg.querySelector('#nd-d' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === ndstep ? '1' : '0'; }
                    if (d) d.setAttribute('fill', i === ndstep ? ndcolors[i] : '#3a3f5c');
                }
                var nh = ndsvg.querySelector('#nd-hint');
                if (nh) nh.textContent = ndstep === ndmax
                    ? 'Click to restart \u2192'
                    : 'Click to step through failure scenario \u2192';
            });
        }

        // Verification modes step-through
        var vmsvg = container.querySelector('#vmode-svg');
        if (vmsvg) {
            var vmstep = 0;
            var vmmax = 2;
            var vmcolors = ['#81d4fa', '#2ecc71', '#ce93d8'];
            vmsvg.addEventListener('click', function() {
                vmstep = (vmstep + 1) % (vmmax + 1);
                for (var i = 0; i <= vmmax; i++) {
                    var g = vmsvg.querySelector('#vm-s' + i);
                    var d = vmsvg.querySelector('#vm-d' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === vmstep ? '1' : '0'; }
                    if (d) d.setAttribute('fill', i === vmstep ? vmcolors[i] : '#3a3f5c');
                }
                var vh = vmsvg.querySelector('#vm-hint');
                if (vh) vh.textContent = vmstep === vmmax
                    ? 'Click to restart \u2192'
                    : 'Click to cycle through modes \u2192';
            });
        }

        // Dual compute pass/fail toggle
        var dusvg = container.querySelector('#dual-svg');
        if (dusvg) {
            var dustate = 0;
            dusvg.addEventListener('click', function() {
                var tr = 'opacity 0.4s';
                dustate = 1 - dustate;
                var s0 = dusvg.querySelector('#du-s0');
                var s1 = dusvg.querySelector('#du-s1');
                s0.style.transition = tr; s1.style.transition = tr;
                s0.style.opacity = dustate === 0 ? '1' : '0';
                s1.style.opacity = dustate === 1 ? '1' : '0';
                var dh = dusvg.querySelector('#du-hint');
                if (dh) dh.textContent = dustate === 0
                    ? 'Click to see failure case \u2192'
                    : '\u2190 Click to see pass case';
            });
        }

        // Verification decision flow step-through
        var flsvg = container.querySelector('#flow-svg');
        if (flsvg) {
            var flstep = 0;
            var flmax = 3;
            var flcolors = ['#81d4fa', '#2ecc71', '#e74c3c', '#ce93d8'];
            flsvg.addEventListener('click', function() {
                flstep = (flstep + 1) % (flmax + 1);
                for (var i = 0; i <= flmax; i++) {
                    var g = flsvg.querySelector('#fl-s' + i);
                    var d = flsvg.querySelector('#fl-d' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === flstep ? '1' : '0'; }
                    if (d) d.setAttribute('fill', i === flstep ? flcolors[i] : '#3a3f5c');
                }
                var fh = flsvg.querySelector('#fl-hint');
                if (fh) fh.textContent = flstep === flmax
                    ? 'Click to restart \u2192'
                    : 'Click to trace each decision path \u2192';
            });
        }

        // Deterministic sampling 3-step
        var sasvg = container.querySelector('#samp-svg');
        if (sasvg) {
            var sastep = 0; var samax = 2;
            sasvg.addEventListener('click', function() {
                sastep = (sastep + 1) % (samax + 1);
                for (var i = 0; i <= samax; i++) {
                    var g = sasvg.querySelector('#sa-s' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === sastep ? '1' : '0'; }
                }
                var h = sasvg.querySelector('#sa-hint');
                if (h) h.textContent = sastep === samax
                    ? 'Click to restart \u2192'
                    : 'Click to cycle: 10% rate \u2192 50% rate \u2192 reproducibility demo \u2192';
            });
        }

        // Wall-clock vs CPU 3-step
        var wlsvg = container.querySelector('#wall-svg');
        if (wlsvg) {
            var wlstep = 0; var wlmax = 2;
            wlsvg.addEventListener('click', function() {
                wlstep = (wlstep + 1) % (wlmax + 1);
                for (var i = 0; i <= wlmax; i++) {
                    var g = wlsvg.querySelector('#wl-s' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === wlstep ? '1' : '0'; }
                }
                var h = wlsvg.querySelector('#wl-hint');
                if (h) h.textContent = wlstep === wlmax
                    ? 'Click to restart \u2192'
                    : 'Click to compare: Off \u2192 Parallel \u2192 Sequential \u2192';
            });
        }

        // Stale data 4-step
        var slsvg = container.querySelector('#stale-svg');
        if (slsvg) {
            var slstep = 0; var slmax = 3;
            slsvg.addEventListener('click', function() {
                slstep = (slstep + 1) % (slmax + 1);
                for (var i = 0; i <= slmax; i++) {
                    var g = slsvg.querySelector('#sl-s' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === slstep ? '1' : '0'; }
                }
                var h = slsvg.querySelector('#sl-hint');
                if (h) h.textContent = slstep === slmax
                    ? 'Click to restart \u2192'
                    : 'Click to step: cached \u2192 naive invalidate \u2192 stale read \u2192 cascade fix \u2192';
            });
        }

        // Cascade architecture 2-state toggle
        var csvg = container.querySelector('#casc-svg');
        if (csvg) {
            var cst = 0;
            var cs0 = csvg.querySelector('#ci-s0');
            var cs1 = csvg.querySelector('#ci-s1');
            csvg.addEventListener('click', function() {
                cst = 1 - cst;
                cs0.style.transition = 'opacity 0.35s'; cs0.style.opacity = cst === 0 ? '1' : '0';
                cs1.style.transition = 'opacity 0.35s'; cs1.style.opacity = cst === 1 ? '1' : '0';
                var h = csvg.querySelector('#ci-hint');
                if (h) h.textContent = cst === 0
                    ? 'Click to toggle: Immediate eviction \u2194 DryRun analysis'
                    : 'Click to toggle: DryRun analysis \u2194 Immediate eviction';
            });
        }

        // Cascade flow 5-step
        var cfsvg = container.querySelector('#cflow-svg');
        if (cfsvg) {
            var cfstep = 0; var cfmax = 4;
            cfsvg.addEventListener('click', function() {
                cfstep = (cfstep + 1) % (cfmax + 1);
                for (var i = 0; i <= cfmax; i++) {
                    var g = cfsvg.querySelector('#cf-s' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === cfstep ? '1' : '0'; }
                }
                var h = cfsvg.querySelector('#cf-hint');
                if (h) h.textContent = cfstep === cfmax
                    ? 'Click to restart \u2192'
                    : 'Click to trace each message in the cascade flow \u2192';
            });
        }

        // Diamond DAG 5-step
        var dmsvg = container.querySelector('#diamond-svg');
        if (dmsvg) {
            var dmstep = 0; var dmmax = 4;
            dmsvg.addEventListener('click', function() {
                dmstep = (dmstep + 1) % (dmmax + 1);
                for (var i = 0; i <= dmmax; i++) {
                    var g = dmsvg.querySelector('#dm-s' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === dmstep ? '1' : '0'; }
                }
                var h = dmsvg.querySelector('#dm-hint');
                if (h) h.textContent = dmstep === dmmax
                    ? 'Click to restart \u2192'
                    : 'Click to step: cached \u2192 invalidate A \u2192 depth 1 \u2192 depth 2 \u2192 result \u2192';
            });
        }

        // Storage growth 3-step
        var grsvg = container.querySelector('#growth-svg');
        if (grsvg) {
            var grstep = 0; var grmax = 2;
            grsvg.addEventListener('click', function() {
                grstep = (grstep + 1) % (grmax + 1);
                for (var i = 0; i <= grmax; i++) {
                    var g = grsvg.querySelector('#gr-s' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === grstep ? '1' : '0'; }
                }
                var h = grsvg.querySelector('#gr-hint');
                if (h) h.textContent = grstep === grmax
                    ? 'Click to restart \u2192'
                    : 'Click to compare: No GC \u2192 Weekly GC \u2192 Side-by-side \u2192';
            });
        }

        // GC mark-sweep 3-step
        var gcsvg = container.querySelector('#gc-svg');
        if (gcsvg) {
            var gcstep = 0; var gcmax = 2;
            gcsvg.addEventListener('click', function() {
                gcstep = (gcstep + 1) % (gcmax + 1);
                for (var i = 0; i <= gcmax; i++) {
                    var g = gcsvg.querySelector('#gc-s' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === gcstep ? '1' : '0'; }
                }
                var h = gcsvg.querySelector('#gc-hint');
                if (h) h.textContent = gcstep === gcmax
                    ? 'Click to restart \u2192'
                    : 'Click to step: Mark \u2192 Sweep \u2192 Report \u2192';
            });
        }

        // GC flow 5-step
        var gfsvg = container.querySelector('#gcflow-svg');
        if (gfsvg) {
            var gfstep = 0; var gfmax = 4;
            gfsvg.addEventListener('click', function() {
                gfstep = (gfstep + 1) % (gfmax + 1);
                for (var i = 0; i <= gfmax; i++) {
                    var g = gfsvg.querySelector('#gf-s' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === gfstep ? '1' : '0'; }
                }
                var h = gfsvg.querySelector('#gf-hint');
                if (h) h.textContent = gfstep === gfmax
                    ? 'Click to restart \u2192'
                    : 'Click to trace: mark \u2192 compute orphans \u2192 sweep \u2192 report \u2192 summary \u2192';
            });
        }

        // Debug slow request 4-step walkthrough
        var dbsvg = container.querySelector('#debug-svg');
        if (dbsvg) {
            var dbstep = 0; var dbmax = 3;
            dbsvg.addEventListener('click', function() {
                dbstep = (dbstep + 1) % (dbmax + 1);
                for (var i = 0; i <= dbmax; i++) {
                    var g = dbsvg.querySelector('#db-s' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === dbstep ? '1' : '0'; }
                }
                var h = dbsvg.querySelector('#db-hint');
                if (h) h.textContent = dbstep === dbmax
                    ? 'Click to restart \u2192'
                    : 'Click: scenario \u2192 metrics \u2192 tracing \u2192 action \u2192';
            });
        }

        // Tradeoff: DAG storage vs speed toggle
        var tdsvg = container.querySelector('#td-dag-svg');
        if (tdsvg) {
            var tdstate = 0;
            tdsvg.addEventListener('click', function() {
                tdstate = 1 - tdstate;
                var s0 = tdsvg.querySelector('#td-s0'); var s1 = tdsvg.querySelector('#td-s1');
                if (s0) { s0.style.transition = 'opacity 0.35s'; s0.style.opacity = tdstate === 0 ? '1' : '0'; }
                if (s1) { s1.style.transition = 'opacity 0.35s'; s1.style.opacity = tdstate === 1 ? '1' : '0'; }
            });
        }

        // Tradeoff: Async complexity vs parallelism toggle
        var tasvg = container.querySelector('#td-async-svg');
        if (tasvg) {
            var tastate = 0;
            tasvg.addEventListener('click', function() {
                tastate = 1 - tastate;
                var s0 = tasvg.querySelector('#ta-s0'); var s1 = tasvg.querySelector('#ta-s1');
                if (s0) { s0.style.transition = 'opacity 0.35s'; s0.style.opacity = tastate === 0 ? '1' : '0'; }
                if (s1) { s1.style.transition = 'opacity 0.35s'; s1.style.opacity = tastate === 1 ? '1' : '0'; }
            });
        }

        // Tradeoff: Verification mode 3-step cycle
        var tvsvg = container.querySelector('#td-verify-svg');
        if (tvsvg) {
            var tvstep = 0; var tvmax = 2;
            tvsvg.addEventListener('click', function() {
                tvstep = (tvstep + 1) % (tvmax + 1);
                for (var i = 0; i <= tvmax; i++) {
                    var g = tvsvg.querySelector('#tv-s' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === tvstep ? '1' : '0'; }
                }
                var h = tvsvg.querySelector('#tv-hint');
                if (h) h.textContent = tvstep === tvmax
                    ? 'Click to restart \u2192'
                    : 'Click: Off \u2192 DualCompute \u2192 Sampled \u2192';
            });
        }

        // Tradeoff: Semaphore placement toggle
        var tssvg = container.querySelector('#td-sem-svg');
        if (tssvg) {
            var tsstate = 0;
            tssvg.addEventListener('click', function() {
                tsstate = 1 - tsstate;
                var s0 = tssvg.querySelector('#ts-s0'); var s1 = tssvg.querySelector('#ts-s1');
                if (s0) { s0.style.transition = 'opacity 0.35s'; s0.style.opacity = tsstate === 0 ? '1' : '0'; }
                if (s1) { s1.style.transition = 'opacity 0.35s'; s1.style.opacity = tsstate === 1 ? '1' : '0'; }
            });
        }

        // Tradeoff: Cascade invalidation toggle
        var tcsvg = container.querySelector('#td-casc-svg');
        if (tcsvg) {
            var tcstate = 0;
            tcsvg.addEventListener('click', function() {
                tcstate = 1 - tcstate;
                var s0 = tcsvg.querySelector('#tc-s0'); var s1 = tcsvg.querySelector('#tc-s1');
                if (s0) { s0.style.transition = 'opacity 0.35s'; s0.style.opacity = tcstate === 0 ? '1' : '0'; }
                if (s1) { s1.style.transition = 'opacity 0.35s'; s1.style.opacity = tcstate === 1 ? '1' : '0'; }
            });
        }

        // Tradeoff: GC frequency 3-step cycle
        var tgsvg = container.querySelector('#td-gc-svg');
        if (tgsvg) {
            var tgstep = 0; var tgmax = 2;
            tgsvg.addEventListener('click', function() {
                tgstep = (tgstep + 1) % (tgmax + 1);
                for (var i = 0; i <= tgmax; i++) {
                    var g = tgsvg.querySelector('#tg-s' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === tgstep ? '1' : '0'; }
                }
                var h = tgsvg.querySelector('#tg-hint');
                if (h) h.textContent = tgstep === tgmax
                    ? 'Click to restart \u2192'
                    : 'Click: No GC \u2192 Hourly \u2192 Weekly \u2192';
            });
        }

        // Tradeoff: Observability toggle
        var tosvg = container.querySelector('#td-obs-svg');
        if (tosvg) {
            var tostate = 0;
            tosvg.addEventListener('click', function() {
                tostate = 1 - tostate;
                var s0 = tosvg.querySelector('#to-s0'); var s1 = tosvg.querySelector('#to-s1');
                if (s0) { s0.style.transition = 'opacity 0.35s'; s0.style.opacity = tostate === 0 ? '1' : '0'; }
                if (s1) { s1.style.transition = 'opacity 0.35s'; s1.style.opacity = tostate === 1 ? '1' : '0'; }
            });
        }

        // Tradeoff: Broadcast channel toggle
        var tbsvg = container.querySelector('#td-bcast-svg');
        if (tbsvg) {
            var tbstate = 0;
            tbsvg.addEventListener('click', function() {
                tbstate = 1 - tbstate;
                var s0 = tbsvg.querySelector('#tb-s0'); var s1 = tbsvg.querySelector('#tb-s1');
                if (s0) { s0.style.transition = 'opacity 0.35s'; s0.style.opacity = tbstate === 0 ? '1' : '0'; }
                if (s1) { s1.style.transition = 'opacity 0.35s'; s1.style.opacity = tbstate === 1 ? '1' : '0'; }
            });
        }

        // Tradeoff: Sled vs alternatives 3-step cycle
        var tlsvg = container.querySelector('#td-sled-svg');
        if (tlsvg) {
            var tlstep = 0; var tlmax = 2;
            tlsvg.addEventListener('click', function() {
                tlstep = (tlstep + 1) % (tlmax + 1);
                for (var i = 0; i <= tlmax; i++) {
                    var g = tlsvg.querySelector('#tl-s' + i);
                    if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === tlstep ? '1' : '0'; }
                }
                var h = tlsvg.querySelector('#tl-hint');
                if (h) h.textContent = tlstep === tlmax
                    ? 'Click to restart \u2192'
                    : 'Click: Sled \u2192 RocksDB \u2192 SQLite \u2192';
            });
        }

        // Parallelism sequential/parallel toggle
        var psvg = container.querySelector('#par-svg');
        if (psvg) {
            var seq = psvg.querySelector('#seq-view');
            var par = psvg.querySelector('#par-view');
            var seqEx = psvg.querySelector('#seq-explain');
            var parEx = psvg.querySelector('#par-explain');
            var phint = psvg.querySelector('#par-hint');
            var pshowing = 'seq';
            psvg.addEventListener('click', function() {
                var t = 'opacity 0.4s';
                seq.style.transition = t; par.style.transition = t;
                seqEx.style.transition = t; parEx.style.transition = t;
                if (pshowing === 'seq') {
                    seq.style.opacity = '0'; par.style.opacity = '1';
                    seqEx.style.opacity = '0'; parEx.style.opacity = '1';
                    phint.textContent = '\u2190 Click to see sequential execution';
                    pshowing = 'par';
                } else {
                    seq.style.opacity = '1'; par.style.opacity = '0';
                    seqEx.style.opacity = '1'; parEx.style.opacity = '0';
                    phint.textContent = 'Click to see parallel execution \u2192';
                    pshowing = 'seq';
                }
            });
        }
    }

    // Load individual sections
    const sections = [
        { id: 'abstract', file: 'abstract.html' },
        { id: 'introduction', file: 'introduction.html' },
        { id: 'background', file: 'background.html' },
        { id: 'persistent-dag', file: 'persistent-dag.html' },
        { id: 'async-materialization', file: 'async-materialization.html' },
        { id: 'verification', file: 'verification.html' },
        { id: 'cascade', file: 'cascade.html' },
        { id: 'gc', file: 'gc.html' },
        { id: 'observability', file: 'observability.html' },
        { id: 'evaluation', file: 'evaluation.html' },
        { id: 'tradeoffs', file: 'tradeoffs.html' },
        { id: 'conclusion', file: 'conclusion.html' }
    ];

    sections.forEach(function(section) {
        fetch(section.file)
            .then(function(response) { return response.text(); })
            .then(function(html) {
                var parser = new DOMParser();
                var doc = parser.parseFromString(html, 'text/html');
                var content = doc.querySelector('.section-content');
                if (content) {
                    var container = document.getElementById(section.id + '-container');
                    container.innerHTML = content.innerHTML;
                    // Apply syntax highlighting + buttons after content is loaded
                    enhanceCodeBlocks(container);
                    // Initialize interactive SVG toggles
                    initDagToggle(container);
                }
            })
            .catch(function(error) {
                console.error('Error loading ' + section.id + ':', error);
                document.getElementById(section.id + '-container').innerHTML = '<p>Error loading content.</p>';
            });
    });
});
