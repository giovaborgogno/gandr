# VHS tape for the gandr README demo.
#   cargo build --release && ./docs/demo-setup.sh && vhs docs/demo.tape
# Drives the real release binary against the /tmp/gandr-demo fixture that
# docs/demo-setup.sh builds. Output: docs/demo.gif.

Output docs/demo.gif

Set Shell "bash"
Set FontSize 16
Set Width 1320
Set Height 820
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 55ms

# --- setup (hidden): enter the demo repo, put the binary on PATH ---
Hide
Type "export PATH=/Users/terac/repos/difftui/target/release:$PATH && cd /tmp/gandr-demo && clear"
Enter
Show

# --- launch: diff tab, file tree + delta-style diff ---
Type "gandr"
Enter
Sleep 2.5s

# walk through the changed files
Type "n"
Sleep 1.6s
Type "n"
Sleep 1.6s
Type "n"
Sleep 1.6s

# focus the diff, scroll, jump hunks
Tab
Sleep 700ms
Type "jjjj"
Sleep 1.2s
Type "]"
Sleep 1.5s

# side-by-side, then back to unified
Type "s"
Sleep 2.5s
Type "s"
Sleep 1.2s

# --- Repo tab: expand the tree, preview updates as the cursor moves ---
Type "2"
Sleep 1.2s
Tab
Sleep 600ms
Type "l"
Sleep 1s
Type "j"
Sleep 900ms
Type "j"
Sleep 900ms
Type "j"
Sleep 1s
Enter
Sleep 2.2s

# repo-wide content search → jump to file + line
Type "/"
Sleep 700ms
Type "cache"
Sleep 1.4s
Enter
Sleep 2.4s
Escape
Sleep 1s

# back to the diff, done
Type "1"
Sleep 1.5s
Type "q"
Sleep 1s
