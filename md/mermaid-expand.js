// Enlarge rendered diagrams without duplicating their SVG IDs or definitions.
(() => {
    const dialog = document.createElement('dialog');
    dialog.className = 'mermaid-expanded';
    dialog.setAttribute('aria-label', 'Expanded diagram');

    const toolbar = document.createElement('div');
    toolbar.className = 'mermaid-expanded-toolbar';
    const hint = document.createElement('span');
    hint.textContent = 'Expanded diagram · scroll to explore';
    const close = document.createElement('button');
    close.type = 'button';
    close.textContent = 'Close';
    close.setAttribute('aria-label', 'Close expanded diagram');
    toolbar.append(hint, close);

    const canvas = document.createElement('div');
    canvas.className = 'mermaid-expanded-canvas';
    dialog.append(toolbar, canvas);
    document.body.append(dialog);

    let active;
    function expand(container) {
        const svg = container.querySelector('svg');
        if (!svg || dialog.open) return;
        active = {
            svg,
            parent: svg.parentNode,
            next: svg.nextSibling,
            style: svg.getAttribute('style'),
            opener: container,
        };
        const bounds = svg.viewBox.baseVal;
        const width = Math.max(bounds.width || svg.getBoundingClientRect().width, window.innerWidth - 96);
        svg.style.width = `${width}px`;
        svg.style.maxWidth = 'none';
        svg.style.height = bounds.width > 0 ? `${width * bounds.height / bounds.width}px` : 'auto';
        canvas.append(svg);
        dialog.showModal();
        document.documentElement.classList.add('mermaid-expanded-open');
        canvas.scrollTop = 0;
        canvas.scrollLeft = 0;
        close.focus();
    }

    close.addEventListener('click', () => dialog.close());
    dialog.addEventListener('click', (event) => {
        const bounds = dialog.getBoundingClientRect();
        if (event.target === dialog && (
            event.clientX < bounds.left || event.clientX > bounds.right ||
            event.clientY < bounds.top || event.clientY > bounds.bottom
        )) dialog.close();
    });
    // Native dialog handles Escape and keeps keyboard focus inside the overlay.
    dialog.addEventListener('close', () => {
        if (!active) return;
        const { svg, parent, next, style, opener } = active;
        if (style === null) svg.removeAttribute('style');
        else svg.setAttribute('style', style);
        parent.insertBefore(svg, next);
        document.documentElement.classList.remove('mermaid-expanded-open');
        active = undefined;
        opener.focus();
    });

    function enableDiagrams() {
        document.querySelectorAll('.mermaid').forEach((container) => {
            if (container.dataset.expandable || !container.querySelector('svg')) return;
            container.dataset.expandable = 'true';
            container.tabIndex = 0;
            container.setAttribute('role', 'button');
            container.setAttribute('aria-label', 'Expand diagram');
            container.setAttribute('aria-haspopup', 'dialog');
            container.title = 'Click to expand diagram';
            container.addEventListener('click', (event) => {
                if (!event.target.closest('a')) expand(container);
            });
            container.addEventListener('keydown', (event) => {
                if (event.target === container && (event.key === 'Enter' || event.key === ' ')) {
                    event.preventDefault();
                    expand(container);
                }
            });
        });
    }

    // Mermaid renders asynchronously, after the book scripts have loaded.
    new MutationObserver(enableDiagrams).observe(document.body, { childList: true, subtree: true });
    enableDiagrams();
})();
