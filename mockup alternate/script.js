document.addEventListener('DOMContentLoaded', () => {
    const navBtns = document.querySelectorAll('.nav-btn');
    const panels = {
        files: document.getElementById('files'),
        ai: document.getElementById('ai')
    };
    const sidebar = document.getElementById('sidebar');
    const editorArea = document.getElementById('editor');

    function handleNavClick(btn) {
        const target = btn.dataset.target;
        if (!target) return;

        // Remove active from all top nav buttons
        document.querySelectorAll('.nav-top .nav-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');

        const isMobile = window.innerWidth < 768;

        if (isMobile) {
            // On mobile, only one view is active
            if (target === 'editor') {
                sidebar.classList.remove('active');
                editorArea.classList.add('active');
            } else {
                editorArea.classList.remove('active');
                sidebar.classList.add('active');
                
                // Switch sidebar panel
                Object.values(panels).forEach(p => {
                    if (p) p.classList.remove('active');
                });
                if (panels[target]) {
                    panels[target].classList.add('active');
                }
            }
        } else {
            // On desktop, editor is always visible, sidebar panels toggle
            if (target !== 'editor') {
                Object.values(panels).forEach(p => {
                    if (p) p.classList.remove('active');
                });
                if (panels[target]) {
                    panels[target].classList.add('active');
                }
            }
        }
    }

    navBtns.forEach(btn => {
        btn.addEventListener('click', () => handleNavClick(btn));
    });
    
    // Handle Window Resize to reset layouts
    window.addEventListener('resize', () => {
        const isMobile = window.innerWidth < 768;
        if (!isMobile) {
            // Desktop reset
            editorArea.classList.add('active');
            sidebar.classList.add('active');
            
            // Ensure editor nav button isn't active on desktop
            const activeNav = document.querySelector('.nav-btn.active');
            if (activeNav && activeNav.dataset.target === 'editor') {
                const filesBtn = document.querySelector('.nav-btn[data-target="files"]');
                if (filesBtn) handleNavClick(filesBtn);
            }
        } else {
            // Mobile reset based on active nav
            const activeNav = document.querySelector('.nav-top .nav-btn.active');
            if (activeNav) {
                handleNavClick(activeNav);
            } else {
                // Default mobile to editor
                const editorBtn = document.querySelector('.nav-btn[data-target="editor"]');
                if (editorBtn) handleNavClick(editorBtn);
            }
        }
    });
    
    // File tree toggle
    const folders = document.querySelectorAll('.tree-item.folder');
    folders.forEach(folder => {
        folder.addEventListener('click', () => {
            folder.classList.toggle('open');
            const children = folder.nextElementSibling;
            if (children && children.classList.contains('tree-children')) {
                children.style.display = folder.classList.contains('open') ? 'block' : 'none';
            }
        });
    });

    // Make Editor Tabs clickable
    const editorTabs = document.querySelectorAll('.editor-tab');
    editorTabs.forEach(tab => {
        tab.addEventListener('click', () => {
            editorTabs.forEach(t => t.classList.remove('active'));
            tab.classList.add('active');
            // Mock changing the file name in breadcrumbs
            const textNodes = Array.from(tab.childNodes).filter(node => node.nodeType === Node.TEXT_NODE);
            const tabName = textNodes.map(node => node.textContent.trim()).join('').trim();
            const breadcrumbLast = document.querySelector('.editor-breadcrumbs span:last-child');
            if (breadcrumbLast && tabName) breadcrumbLast.textContent = tabName;
        });
    });
    
    // Initialize layout properly based on initial screen size
    if (window.innerWidth < 768) {
        // Find active tab or default to files
        const activeNav = document.querySelector('.nav-top .nav-btn.active') || document.querySelector('.nav-btn[data-target="files"]');
        if (activeNav) handleNavClick(activeNav);
    }
});
