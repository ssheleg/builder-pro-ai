# Builder Pro AI bash shell integration (OSC 133 + OSC 7). Loaded via `bash --init-file <this>`.
# Sources the user's rc FIRST, then wraps PROMPT_COMMAND and installs a guarded preexec.
# Emit order per spec §10.2:
#   PROMPT_COMMAND: capture $? first -> D;<code> -> A -> OSC 7 ; B lives at end of PS1 (\[ \]).
#   preexec/DEBUG : C exactly once (functrace-enabled so subshells fire it too — see below).

if [ -n "${_bpa_loaded-}" ]; then return; fi
_bpa_loaded=1

# --- source the user's real rc first (interactive non-login) --------------
if [ -f "$HOME/.bashrc" ]; then
  source "$HOME/.bashrc"
fi

# --- emit helpers ---------------------------------------------------------
_bpa_osc133() { printf '\033]133;%s\007' "$1"; }
_bpa_osc133_d() { printf '\033]133;D;%s\007' "$1"; }
_bpa_osc7() { printf '\033]7;file://%s%s\007' "${HOSTNAME:-localhost}" "$PWD"; }

# --- PROMPT_COMMAND wrapper (never clobber; run the user's original) ------
_bpa_orig_prompt_command="$PROMPT_COMMAND"
_bpa_prompt() {
  local code=$?                 # MUST be first
  # Run the user's original PROMPT_COMMAND (string form) preserving $? for them.
  if [ -n "$_bpa_orig_prompt_command" ]; then
    ( exit "$code" ); eval "$_bpa_orig_prompt_command"
  fi
  _bpa_osc133_d "$code"         # D;<code>
  _bpa_osc133 A                 # A
  _bpa_osc7                     # OSC 7
  _bpa_preexec_ran=""           # re-arm the preexec guard for the next command
  _bpa_inside_prompt=""         # lower the reentrancy guard raised by _bpa_preexec on entry
}
PROMPT_COMMAND=_bpa_prompt

# --- embed B at the END of PS1, wrapped in \[ \] so bash does not miscount -
PS1="${PS1}\[$(_bpa_osc133 B)\]"

# --- preexec: bash-preexec if present, else a guarded DEBUG trap ----------
_bpa_preexec_ran=""
_bpa_inside_prompt=""            # reentrancy guard: DEBUG must not fire for PROMPT_COMMAND internals
_bpa_preexec() {
  # Fire C exactly once per command; never for PROMPT_COMMAND itself, and (with functrace enabled,
  # see below) never for any command running INSIDE it either — once functrace is on, DEBUG fires
  # for every simple command inside a traced function body, not just the top-level call to that
  # function, so a plain `[ "$BASH_COMMAND" = "$PROMPT_COMMAND" ]` check only catches the FIRST of
  # those firings (the call boundary itself). The fix: the moment we see that first firing, raise
  # `_bpa_inside_prompt` right here (BEFORE `_bpa_prompt`'s body starts executing) so every
  # subsequent DEBUG firing for a statement inside `_bpa_prompt` is suppressed too;
  # `_bpa_prompt` lowers the guard again as its last statement once it's done.
  if [ -n "$COMP_LINE" ]; then return; fi         # skip completion
  if [ -n "$_bpa_inside_prompt" ]; then return; fi # skip anything inside our PROMPT_COMMAND
  if [ "$BASH_COMMAND" = "$PROMPT_COMMAND" ]; then
    _bpa_inside_prompt=1
    return
  fi
  if [ -n "$_bpa_preexec_ran" ]; then return; fi
  _bpa_preexec_ran=1
  _bpa_osc133 C
}

# Enable functrace so the DEBUG trap (and bash-preexec, which itself expects functrace) is
# inherited by subshells `( ... )`, shell functions, and command substitutions — not just
# top-level simple commands in the current shell. Without this, bash does NOT propagate DEBUG
# into a subshell, so a command whose top-level form is a subshell (e.g. `(exit 37)`,
# `(cd /tmp && echo hi)`, `(export X=1; cmd)`) never fires the trap at all, and `133;C` (the
# "command running" marker) silently never emits for that whole class of real, common commands.
# The existing `_bpa_preexec_ran` once-per-command guard (re-armed each PROMPT_COMMAND cycle)
# still yields exactly one `C` per top-level command even though DEBUG now fires for every
# simple command inside compound/subshell forms too.
set -o functrace

if [ -n "${bash_preexec_imported:-}" ] || [ -n "${__bp_imported:-}" ]; then
  # bash-preexec is loaded: register into its array instead of a raw trap.
  preexec_functions+=(_bpa_preexec)
else
  # Chain any pre-existing DEBUG trap, then add ours.
  _bpa_prev_debug_trap="$(trap -p DEBUG | sed -E "s/^trap -- '(.*)' DEBUG$/\1/")"
  if [ -n "$_bpa_prev_debug_trap" ]; then
    trap "${_bpa_prev_debug_trap}; _bpa_preexec" DEBUG
  else
    trap '_bpa_preexec' DEBUG
  fi
fi
