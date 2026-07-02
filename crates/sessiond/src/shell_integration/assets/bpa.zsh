# Builder Pro AI zsh shell integration (OSC 133 + OSC 7). Sourced from the ZDOTDIR .zshenv stub
# AFTER the user's real startup files. Non-invasive: no user rc edits.
# Emit order per spec §10.2:
#   precmd : capture $? first -> D;<code> -> A -> OSC 7 ; B lives at end of PS1 (zero-width).
#   preexec: C exactly once.

# Guard against double-load.
if [ -n "${_bpa_loaded-}" ]; then return; fi
_bpa_loaded=1

# --- emit helpers ---------------------------------------------------------
_bpa_osc133() { printf '\033]133;%s\007' "$1"; }             # A | B | C
_bpa_osc133_d() { printf '\033]133;D;%s\007' "$1"; }          # D;<exit>
_bpa_osc7() { printf '\033]7;file://%s%s\007' "${HOST:-localhost}" "$PWD"; }

# --- precmd: close prev command, start new prompt, report cwd -------------
_bpa_precmd() {
  local code=$?                 # MUST be first: the previous command's exit status
  _bpa_osc133_d "$code"         # D;<code>
  _bpa_osc133 A                 # A prompt start
  _bpa_osc7                     # OSC 7 cwd
  # Re-assert the B marker at the end of PS1 on every prompt render. This must live in precmd
  # (not a one-time append at source time) because `.zshenv` runs BEFORE `/etc/zshrc` and the
  # user's `.zshrc`, either of which may reassign PS1 wholesale and would otherwise clobber a
  # source-time append. %{...%} tells zsh the enclosed bytes are non-printing (zero-width) so
  # line-length/wrap math stays correct. Idempotent via a remembered suffix: only append when
  # PS1 does not already end with the marker we last applied (covers both a fresh/reassigned
  # PS1 and a PS1 that already carries our marker from a previous precmd run).
  if [ -z "${_bpa_ps1_marker-}" ]; then
    _bpa_ps1_marker="%{$(_bpa_osc133 B)%}"
  fi
  case "$PS1" in
    *"$_bpa_ps1_marker") ;; # already current, nothing to do
    *) PS1="${PS1}${_bpa_ps1_marker}" ;;
  esac
}

# --- preexec: command dispatched, output begins --------------------------
_bpa_preexec() {
  _bpa_osc133 C                 # C exactly once per command
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd _bpa_precmd
add-zsh-hook preexec _bpa_preexec
