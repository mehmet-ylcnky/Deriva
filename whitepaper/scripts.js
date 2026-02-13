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
                }
            })
            .catch(error => {
                console.error('Error loading ' + section.id + ':', error);
                document.getElementById(section.id + '-container').innerHTML = '<p>Error loading content.</p>';
            });
    });
});
