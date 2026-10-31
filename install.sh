#!/usr/bin/env bash
# COMPLETE-ME installer: everything a first-time user needs, in one go. Safe to re-run.
#
#   ./install.sh                  interactive: checks deps, offers to install them, hooks up your shells,
#                                 runs a self-test and shows a short tour
#   ./install.sh --yes            same, answering yes to everything (non-interactive)
#   ./install.sh --no-deps        don't install packages; just link and hook up
#   ./install.sh --shells fish    only these shells (space or comma separated: fish bash zsh)
#   ./install.sh --dry-run        print what would happen, change nothing
#   ./install.sh --with-blesh     bash: also install ble.sh (fish-style suggestions + highlighting for bash)
#   ./install.sh --uninstall      remove everything this script added (generated caches are kept)
#
#   curl -fsSL <raw url>/install.sh | bash     clones into ~/.local/share/complete-me, then installs
#
# What it changes, and nothing else:
#   packages (only with your OK)                   rust/cargo (to build), bubblewrap, fzf, a clipboard reader,
#                                                  bash-completion (bash), zsh-autosuggestions + zsh-syntax-highlighting (zsh)
#   ~/.local/bin/complete-me                       the compiled binary (cargo install --root ~/.local)
#   ~/.config/fish/conf.d/complete_me.fish         one guarded `source` line
#   ~/.bashrc, ~/.zshrc                            one marked line each, only if the file exists, with your OK
#                                                  (--with-blesh: two more marked lines in ~/.bashrc, ble.sh in ~/.local/share/blesh)
set -euo pipefail

REPO_URL="${COMPLETE_ME_REPO:-https://github.com/arf-io/COMPLETE-ME.git}"
BIN_DIR="$HOME/.local/bin"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}"
FISH_CONF="$CONF/fish/conf.d/complete_me.fish"
MARK="# complete-me"

YES=0 DEPS=1 DRY=0 SHELLS="fish bash zsh" UNINSTALL=0 BLESH=0
while [ $# -gt 0 ]; do
    case "$1" in
        -y|--yes) YES=1 ;;
        --no-deps) DEPS=0 ;;
        --dry-run) DRY=1 ;;
        --shells) SHELLS="${2//,/ }"; shift ;;
        --uninstall) UNINSTALL=1 ;;
        --with-blesh) BLESH=1 ;;
        -h|--help) sed -n '2,22p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
    esac
    shift
done

# ------------------------------------------------------------------ helpers
if [ -t 1 ]; then B=$'\e[1m' G=$'\e[32m' Y=$'\e[33m' R=$'\e[31m' D=$'\e[2m' X=$'\e[0m'; else B= G= Y= R= D= X=; fi
step() { printf '\n%s==>%s %s%s%s\n' "$G" "$X" "$B" "$*" "$X"; }
ok()   { printf '  %s✓%s %s\n' "$G" "$X" "$*"; }
warn() { printf '  %s!%s %s\n' "$Y" "$X" "$*"; }
fail() { printf '  %s✗%s %s\n' "$R" "$X" "$*" >&2; }
run()  { if [ "$DRY" = 1 ]; then printf '  %s[dry-run]%s %s\n' "$D" "$X" "$*"; else "$@"; fi; }
have() { command -v "$1" >/dev/null 2>&1; }
want_shell() { case " $SHELLS " in *" $1 "*) return 0;; esac; return 1; }
ask() {
    [ "$YES" = 1 ] || [ "$DRY" = 1 ] && return 0
    if { true </dev/tty; } 2>/dev/null; then
        printf '  %s?%s %s [Y/n] ' "$Y" "$X" "$1" >/dev/tty
        local a; read -r a </dev/tty || a=n
        case "$a" in ""|y|Y|yes|YES) return 0;; esac
    fi
    return 1
}

# ------------------------------------------------------------------ where is the checkout?
SELF="${BASH_SOURCE[0]:-}"
if [ -n "$SELF" ] && [ -f "$SELF" ] && [ -f "$(dirname "$SELF")/Cargo.toml" ]; then
    REPO="$(cd "$(dirname "$SELF")" && pwd -P)"
else
    REPO="${COMPLETE_ME_HOME:-$HOME/.local/share/complete-me}"
fi

if [ "$UNINSTALL" = 1 ]; then
    step "Uninstalling COMPLETE-ME"
    if have cargo && [ -f "$HOME/.local/.crates.toml" ] && grep -q '^"complete-me ' "$HOME/.local/.crates.toml"; then
        run cargo uninstall --quiet --root "$HOME/.local" complete-me || true
    fi
    run rm -f "$BIN_DIR/complete-me" "$FISH_CONF"
    for rc in "$HOME/.bashrc" "$HOME/.zshrc"; do
        if [ -f "$rc" ] && grep -q "$MARK\$" "$rc"; then
            run sed -i.bak "/$MARK\$/d" "$rc"; ok "cleaned $rc (backup: $rc.bak)"
        fi
    done
    ok "removed. Generated completions remain in ${XDG_CACHE_HOME:-$HOME/.cache}/complete-me; packages were left installed."
    exit 0
fi

printf '%sCOMPLETE-ME%s — tab completion for everything\n' "$B" "$X"
[ "$DRY" = 1 ] && warn "dry run: nothing will be changed"

# ------------------------------------------------------------------ platform
step "Checking your system"
OS="$(uname -s)"
PM=""
BREW="$(command -v brew || { [ -x /home/linuxbrew/.linuxbrew/bin/brew ] && echo /home/linuxbrew/.linuxbrew/bin/brew; } || { [ -x /opt/homebrew/bin/brew ] && echo /opt/homebrew/bin/brew; } || true)"
if [ "$OS" = Darwin ]; then
    [ -n "$BREW" ] && PM=brew
else
    for p in pacman apt-get dnf zypper apk xbps-install; do have "$p" && { PM=$p; break; }; done
fi
ok "OS: $OS${PM:+, package manager: $PM}${BREW:+, Homebrew available}"

SUDO=""
[ "$(id -u)" != 0 ] && { have sudo && SUDO=sudo || { have doas && SUDO=doas; }; }
root_ok() {   # can we run the system package manager as root now (or prompt for it)?
    [ "$PM" = brew ] && return 0
    [ "$(id -u)" = 0 ] && return 0
    [ -n "$SUDO" ] || return 1
    $SUDO -n true 2>/dev/null && return 0
    { true </dev/tty; } 2>/dev/null
}
pkg_name() {
    case "$PM:$1" in
        pacman:cargo|brew:cargo|dnf:cargo|zypper:cargo|apk:cargo|xbps-install:cargo) [ "$PM" = pacman ] || [ "$PM" = brew ] && echo rust || echo cargo ;;
        apt-get:cargo) echo cargo ;;
        brew:bash-completion) echo bash-completion@2 ;;
        *:bwrap) echo bubblewrap ;;
        *:wl-paste) echo wl-clipboard ;;
        *) echo "$1" ;;
    esac
}
sys_install() {
    case "$PM" in
        pacman)       run $SUDO pacman -S --needed --noconfirm "$@" ;;
        apt-get)      run $SUDO apt-get install -y "$@" ;;
        dnf)          run $SUDO dnf install -y "$@" ;;
        zypper)       run $SUDO zypper --non-interactive install "$@" ;;
        apk)          run $SUDO apk add "$@" ;;
        xbps-install) run $SUDO xbps-install -y "$@" ;;
        brew)         run "$BREW" install "$@" ;;
        *) return 1 ;;
    esac
}

# ------------------------------------------------------------------ dependencies
step "Dependencies"
sys_need=()
dep() {   # dep command "why" required
    if have "$1"; then ok "$1 — $2"; return; fi
    if [ "$3" = 1 ]; then fail "$1 missing — $2 (required)"; else warn "$1 missing — $2"; fi
    sys_need+=("$(pkg_name "$1")")
}
dep cargo "builds complete-me (Rust)" 1
dep git "fetches COMPLETE-ME" 0
[ "$OS" != Darwin ] && dep bwrap "sandbox for --help probing (strongly recommended)" 0
dep fzf "live regex builder (alt-r) and snippet picker (alt-j)" 0
if [ "$OS" != Darwin ]; then
    if [ -n "${WAYLAND_DISPLAY:-}" ]; then dep wl-paste "clipboard-aware completion" 0
    elif [ -n "${DISPLAY:-}" ] && ! have xsel; then dep xclip "clipboard-aware completion" 0; fi
fi
have fish || warn "fish not found — bash and zsh get the same extras (history, clipboard, regex, find, ssh, alt-keys)"
# bash: bash-completion is what most completions live in; complete-me layers on top of it.
bash_completion_file() {
    for f in /usr/share/bash-completion/bash_completion /usr/local/share/bash-completion/bash_completion \
        /opt/homebrew/share/bash-completion/bash_completion /home/linuxbrew/.linuxbrew/share/bash-completion/bash_completion; do
        [ -r "$f" ] && { echo "$f"; return 0; }
    done
    return 1
}
if want_shell bash && [ -f "$HOME/.bashrc" ]; then
    if bash_completion_file >/dev/null; then ok "bash-completion — completions for git, ssh, … (complete-me adds its extras on top)"
    else warn "bash-completion missing — git/ssh/… completions for bash"; sys_need+=(bash-completion); fi
fi
# zsh: fish-style gray suggestions from history (→ accepts) and command colouring.
zsh_plugin() {
    for d in /usr/share/zsh/plugins /usr/share /usr/local/share /opt/homebrew/share /home/linuxbrew/.linuxbrew/share \
        "${XDG_DATA_HOME:-$HOME/.local/share}/zsh/plugins"; do
        [ -r "$d/$1/$1.zsh" ] && return 0
    done
    return 1
}
if want_shell zsh && have zsh && [ -f "$HOME/.zshrc" ]; then
    for p in zsh-autosuggestions zsh-syntax-highlighting; do
        if zsh_plugin "$p"; then ok "$p — the fish feel in zsh (complete-me loads it)"
        else warn "$p missing — the fish feel in zsh: $([ $p = zsh-autosuggestions ] && echo 'gray suggestions from history' || echo 'command colouring')"; sys_need+=("$p"); fi
    done
fi

if [ "$DEPS" = 1 ] && [ "${#sys_need[@]}" -gt 0 ]; then
    if [ -n "$PM" ] && root_ok && ask "Install ${sys_need[*]} with $PM?"; then
        sys_install "${sys_need[@]}" && ok "installed ${sys_need[*]}" || warn "$PM failed; continuing"
    else
        # No root here: Homebrew can still provide these without sudo.
        rest=() brewable=()
        for p in "${sys_need[@]}"; do
            case "$p" in fzf|zsh-autosuggestions|zsh-syntax-highlighting) [ -n "$BREW" ] && brewable+=("$p") || rest+=("$p") ;;
                bash-completion) [ -n "$BREW" ] && brewable+=(bash-completion@2) || rest+=("$p") ;;
                *) rest+=("$p") ;; esac
        done
        if [ "${#brewable[@]}" -gt 0 ] && ask "No sudo available — install ${brewable[*]} with Homebrew instead?"; then
            run "$BREW" install "${brewable[@]}" && ok "installed via Homebrew: ${brewable[*]}" || rest+=("${brewable[@]}")
        elif [ "${#brewable[@]}" -gt 0 ]; then rest+=("${brewable[@]}"); fi
        [ "${#rest[@]}" -gt 0 ] && warn "install these yourself when you can:  ${SUDO:+$SUDO }${PM:-<package manager>} install ${rest[*]}"
    fi
fi
have cargo || { fail "cargo (Rust) is required to build complete-me: https://rustup.rs  (or your package manager's rust)"; exit 1; }

# ------------------------------------------------------------------ checkout
if [ ! -f "$REPO/Cargo.toml" ]; then
    step "Fetching COMPLETE-ME"
    have git || { fail "git is needed to fetch COMPLETE-ME"; exit 1; }
    run mkdir -p "$(dirname "$REPO")"
    run git clone --depth 1 "$REPO_URL" "$REPO"
    ok "cloned into $REPO"
fi

# ------------------------------------------------------------------ install
step "Installing"
run mkdir -p "$BIN_DIR"
if [ "$DRY" = 1 ]; then
    echo "  [dry-run] cargo install --path $REPO --root $HOME/.local --locked"
else
    # An older install may have left a symlink here; cargo must not write through it.
    [ -L "$BIN_DIR/complete-me" ] && rm -f "$BIN_DIR/complete-me"
    echo "  building (release)… first build takes a minute"
    if ! cargo install --quiet --path "$REPO" --root "$HOME/.local" --locked --force 2>&1 | tail -20; then
        fail "build failed"; exit 1
    fi
fi
ok "$BIN_DIR/complete-me ($( [ "$DRY" = 1 ] && echo "would be built" || "$BIN_DIR/complete-me" --version))"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *)
    warn "$BIN_DIR is not on your PATH"
    if have fish && want_shell fish; then run fish -c "fish_add_path -U $BIN_DIR" && ok "added it to fish's PATH"; fi
    warn "for bash/zsh add:  export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
esac

hooked=()
if want_shell fish && have fish; then
    run mkdir -p "$(dirname "$FISH_CONF")"
    if [ "$DRY" = 1 ]; then echo "  [dry-run] write $FISH_CONF"; else
        # Guarded: if the checkout moves or a branch without it is checked out, shells still start cleanly.
        printf '%s\ntest -f %q; and source %q\n' "$MARK: generated by install.sh; remove with ./install.sh --uninstall" \
            "$REPO/shell/fish/complete_me.fish" "$REPO/shell/fish/complete_me.fish" > "$FISH_CONF"
    fi
    ok "fish: $FISH_CONF"; hooked+=(fish)
fi
for shell in bash zsh; do
    want_shell "$shell" || continue
    rc="$HOME/.${shell}rc"
    [ -f "$rc" ] || continue
    line="[ -f '$REPO/shell/$shell/complete_me.$shell' ] && source '$REPO/shell/$shell/complete_me.$shell'  $MARK"
    if grep -qF "$MARK" "$rc"; then ok "$shell: already hooked in $rc"
    elif ask "Add one line to $rc for $shell support?"; then
        if [ "$DRY" = 1 ]; then echo "  [dry-run] append to $rc"; else printf '\n%s\n' "$line" >> "$rc"; fi
        ok "$shell: $rc"; hooked+=("$shell")
    fi
done
# bash: ble.sh gives bash fish-style suggestions and highlighting. It must load first in ~/.bashrc
# and attach last, so it gets two marked lines: one at the very top, one at the very bottom.
if [ "$BLESH" = 1 ] && want_shell bash && [ -f "$HOME/.bashrc" ]; then
    blesh="$HOME/.local/share/blesh/ble.sh"
    if [ ! -f "$blesh" ]; then
        if have git && have make && ask "Fetch and build ble.sh (github.com/akinomyoga/ble.sh) into ~/.local/share/blesh?"; then
            tmp="$(mktemp -d)"
            run git clone --recursive --depth 1 --shallow-submodules https://github.com/akinomyoga/ble.sh.git "$tmp/ble.sh" &&
                run make -C "$tmp/ble.sh" install PREFIX="$HOME/.local" >/dev/null && ok "ble.sh installed" || warn "ble.sh build failed"
            rm -rf "$tmp"
        else
            warn "ble.sh needs git and make"
        fi
    fi
    if [ -f "$blesh" ] || [ "$DRY" = 1 ]; then
        if grep -q "ble.sh --noattach.*$MARK\$" "$HOME/.bashrc" 2>/dev/null; then ok "bash: ble.sh already hooked"
        elif [ "$DRY" = 1 ]; then echo "  [dry-run] add ble.sh lines to the top and bottom of ~/.bashrc"
        else
            { printf '%s\n' "[[ \$- == *i* ]] && source '$blesh' --noattach  $MARK"; cat "$HOME/.bashrc"
              printf '\n%s\n' "[[ \${BLE_VERSION-} ]] && ble-attach  $MARK"; } > "$HOME/.bashrc.cm.tmp" &&
                cp "$HOME/.bashrc" "$HOME/.bashrc.bak" && mv "$HOME/.bashrc.cm.tmp" "$HOME/.bashrc"
            ok "bash: ble.sh hooked in ~/.bashrc (backup: ~/.bashrc.bak)"
        fi
    fi
fi
[ "${#hooked[@]}" -eq 0 ] && warn "no shell hooked up (fish not installed and no ~/.bashrc / ~/.zshrc?)"

# ------------------------------------------------------------------ self-test
step "Self-test"
if [ "$DRY" = 1 ]; then echo "  [dry-run] would check the sandbox and parse ls"; else
    if have bwrap; then
        if bwrap --ro-bind / / --dev /dev --proc /proc --tmpfs /tmp --tmpfs /run --unshare-all --die-with-parent -- true 2>/dev/null; then
            ok "sandbox works (read-only, no network, private /tmp and /run)"
        else
            warn "bwrap can't create a sandbox here (user namespaces off?): scripts will be read, never run"
        fi
    else
        warn "no sandbox: scripts are read, never run; binaries get --help only (deny-list applies)"
    fi
    "$BIN_DIR/complete-me" show ls >/dev/null 2>&1 && ok "parsed 'ls' as a smoke test" || warn "'complete-me show ls' failed"
    ok "full test suite: make test"
fi

# ------------------------------------------------------------------ tour
step "Done. Open a new terminal, then try:"
cat <<EOF
  ${B}<TAB>${X} on any command          completions appear even for tools that never shipped any
  ${B}git clone <TAB>${X}              a git URL you just copied is offered first
  ${B}grep -E 'err<TAB>${X}             regex pieces, each with what it means
  ${B}alt-r${X}                         live regex builder: type, watch matches, ENTER to insert
  ${B}alt-x${X}                         explain the command line (find expressions, regexes, flags)
  ${B}alt-j${X}                         snippets + bash→fish phrasebook   (or type ${B},for${X} then space)
  ${B}for i in {1..3}; do echo \$i; done${X}   bash typed into fish is translated, checked, and run
  ${B}find . -exec rm <TAB>${X}         {}  then  \\;  or  +
  ${B}complete-me alias gsw git switch${X}   aliases that keep their completions

  bash and zsh get the same TAB extras, alt-keys and ,snippets (in their own syntax). zsh also
  gets gray suggestions from history (→ accepts) via zsh-autosuggestions; for bash, re-run with
  ${B}--with-blesh${X}.

  Uninstall any time:  $REPO/install.sh --uninstall
EOF
