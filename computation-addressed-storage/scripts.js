document.addEventListener('DOMContentLoaded', function() {
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

    // Copy button + dark theme toggle for code blocks
    document.querySelectorAll('.code-block').forEach(block => {
        const copyBtn = document.createElement('button');
        copyBtn.textContent = 'Copy';
        copyBtn.className = 'copy-btn';

        const themeBtn = document.createElement('button');
        themeBtn.textContent = '🌙';
        themeBtn.className = 'theme-btn';
        themeBtn.title = 'Toggle dark/light theme';

        block.appendChild(copyBtn);
        block.appendChild(themeBtn);

        copyBtn.addEventListener('click', () => {
            const text = block.querySelector('pre').textContent;
            navigator.clipboard.writeText(text).then(() => {
                copyBtn.textContent = 'Copied!';
                setTimeout(() => { copyBtn.textContent = 'Copy'; }, 2000);
            });
        });

        themeBtn.addEventListener('click', () => {
            block.classList.toggle('dark-theme');
            themeBtn.textContent = block.classList.contains('dark-theme') ? '☀️' : '🌙';
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

    // Load individual sections
    const sections = [
        { id: 'abstract', file: 'abstract.html' },
        { id: 'introduction', file: 'introduction.html' },
        { id: 'motivation', file: 'motivation.html' },
        { id: 'prior-art', file: 'prior-art.html' },
        { id: 'core-concepts', file: 'core-concepts.html' },
        { id: 'architecture', file: 'architecture.html' },
        { id: 'implementation', file: 'implementation.html' },
        { id: 'evaluation', file: 'evaluation.html' },
        { id: 'tradeoffs', file: 'tradeoffs.html' },
        { id: 'discussion', file: 'discussion.html' },
        { id: 'future-work', file: 'future-work.html' },
        { id: 'conclusion', file: 'conclusion.html' },
        { id: 'references', file: 'references.html' }
    ];

    sections.forEach(section => {
        fetch(section.file)
            .then(response => response.text())
            .then(html => {
                const parser = new DOMParser();
                const doc = parser.parseFromString(html, 'text/html');
                const content = doc.querySelector('.section-content');
                if (content) {
                    document.getElementById(section.id + '-container').innerHTML = content.innerHTML;
                    // Re-attach code block buttons after loading
                    document.querySelectorAll('#' + section.id + '-container .code-block').forEach(block => {
                        if (!block.querySelector('.copy-btn')) {
                            const cb = document.createElement('button');
                            cb.textContent = 'Copy'; cb.className = 'copy-btn';
                            const tb = document.createElement('button');
                            tb.textContent = '🌙'; tb.className = 'theme-btn'; tb.title = 'Toggle theme';
                            block.appendChild(cb); block.appendChild(tb);
                            cb.addEventListener('click', () => {
                                const t = block.querySelector('pre').textContent;
                                navigator.clipboard.writeText(t).then(() => {
                                    cb.textContent = 'Copied!';
                                    setTimeout(() => { cb.textContent = 'Copy'; }, 2000);
                                });
                            });
                            tb.addEventListener('click', () => {
                                block.classList.toggle('dark-theme');
                                tb.textContent = block.classList.contains('dark-theme') ? '☀️' : '🌙';
                            });
                        }
                    });
                    // Initialize SVG interactive handlers
                    initSvgHandlers(document.getElementById(section.id + '-container'));
                }
            })
            .catch(error => {
                console.error('Error loading ' + section.id + ':', error);
                document.getElementById(section.id + '-container').innerHTML = '<p>Error loading content.</p>';
            });
    });
});

function initSvgHandlers(container) {
    if (!container) return;

    // Materialization flow — 6-step walkthrough
    var mf = container.querySelector('#mat-flow-svg');
    if (mf) {
        var mfStep = 0; var mfMax = 5;
        mf.addEventListener('click', function() {
            mfStep = (mfStep + 1) % (mfMax + 1);
            for (var i = 0; i <= mfMax; i++) {
                var g = mf.querySelector('#mf-s' + i);
                if (g) { g.style.transition = 'opacity 0.35s'; g.style.opacity = i === mfStep ? '1' : '0'; }
            }
            var h = mf.querySelector('#mf-hint');
            if (h) h.textContent = mfStep === 0
                ? 'Click to step through materialization flow (Step 1 of 6)'
                : 'Step ' + (mfStep + 1) + ' of 6' + (mfStep === mfMax ? ' — click to restart' : '');
        });
    }

    // Architecture layers — 4-step highlight
    var al = container.querySelector('#arch-layers-svg');
    if (al) {
        var alStep = 0; var alMax = 4;
        var alGroups = ['al-client', 'al-service', 'al-middle', 'al-core'];
        al.addEventListener('click', function() {
            alStep = (alStep + 1) % (alMax + 1);
            // Dim/brighten layer groups
            alGroups.forEach(function(gid, idx) {
                var g = al.querySelector('#' + gid);
                if (g) g.style.opacity = (alStep === 0 || idx === alStep - 1) ? '1' : '0.3';
            });
            // Toggle info panels
            for (var i = 0; i <= alMax; i++) {
                var s = al.querySelector('#al-s' + i);
                if (s) { s.style.transition = 'opacity 0.35s'; s.style.opacity = i === alStep ? '1' : '0'; }
            }
            var h = al.querySelector('#al-hint');
            if (h) h.textContent = alStep === 0
                ? 'Click to highlight layers (0 of 4)'
                : 'Layer ' + alStep + ' of 4' + (alStep === alMax ? ' — click to reset' : '');
        });
    }

    // Crate dependency graph — 6-step highlight
    var cd = container.querySelector('#crate-dep-svg');
    if (cd) {
        var cdStep = 0; var cdMax = 6;
        var cdBoxes = ['cd-cli', 'cd-server', 'cd-compute', 'cd-storage', 'cd-core', 'cd-network'];
        cd.addEventListener('click', function() {
            cdStep = (cdStep + 1) % (cdMax + 1);
            // Dim/brighten crate boxes
            cdBoxes.forEach(function(bid, idx) {
                var b = cd.querySelector('#' + bid);
                if (b) b.style.opacity = (cdStep === 0 || idx === cdStep - 1) ? '1' : '0.3';
            });
            // Toggle info panels
            for (var i = 0; i <= cdMax; i++) {
                var s = cd.querySelector('#cd-s' + i);
                if (s) { s.style.transition = 'opacity 0.35s'; s.style.opacity = i === cdStep ? '1' : '0'; }
            }
            var h = cd.querySelector('#cd-hint');
            if (h) h.textContent = cdStep === 0
                ? 'Click to cycle through crates (0 of 6)'
                : 'Crate ' + cdStep + ' of 6' + (cdStep === cdMax ? ' — click to reset' : '');
        });
    }
}
