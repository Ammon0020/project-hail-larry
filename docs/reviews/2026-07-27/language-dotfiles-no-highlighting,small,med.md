- name: language.ts extOf returns '' for dotfiles — .bashrc/.eslintrc/.vimrc get no highlighting
- file: /media/adam/extex/projects/project-hail-larry/web/src/lib/language.ts
- lines: 24-30, 45-71
- description: |
    `extOf` (line 28) returns `''` when `dot <= 0`, which is true for both
    extensionless files AND dotfiles (where the only dot is at index 0).
    So `.bashrc`, `.vimrc`, `.eslintrc`, `.zshrc`, `.gitconfig` get `ext === ''`,
    skip all the explicit language branches (shell, etc.), and fall through
    to `languageDescriptionForPath(path)` (line 68).

    That fallback uses `LanguageDescription.matchFilename(mdLanguages, basename)`
    which matches by filename pattern — so whether `.bashrc` gets shell
    highlighting depends entirely on whether `@codemirror/language-data`
    happens to have an entry for that exact dotfile name. Many common
    dotfiles (`.vimrc`, `.gitconfig`, `.bash_profile`, `.inputrc`) are not
    in language-data, so they open with no syntax highlighting and no
    lazy-load trigger, leaving the user with a plain-text editor for files
    that are clearly shell/config code.

    User-facing impact: opening a `.bashrc` or `.vimrc` shows monochrome
    text with no highlighting, which reads as "the editor doesn't support
    this file" rather than "we don't recognize the extension."

    Fix: in `extOf`, treat a leading-dot basename with no further dot as
    having the full basename (minus the dot) as its "extension" for
    matching purposes (so `.bashrc` → `bashrc`), OR add an explicit
    dotfile → language map for the common cases (`.bashrc`/`.bash_profile`/
    `.zshrc`/`.profile` → shell, `.vimrc` → vim, `.gitconfig` → ini/toml,
    `.eslintrc` → json). The latter is more predictable.
- verification: |
    Read language.ts in full. `extOf` line 28 `if (dot <= 0) return ''`
    covers dotfiles. The explicit branches at lines 47-64 only check
    known extensions, so dotfiles rely entirely on the language-data
    fallback at line 68.
