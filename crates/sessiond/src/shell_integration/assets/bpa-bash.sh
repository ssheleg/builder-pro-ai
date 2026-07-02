# Builder Pro AI bash shell integration (OSC 133 + OSC 7). Loaded via `bash --init-file <this>`.
# Sources the user's rc FIRST, then wraps PROMPT_COMMAND and installs a guarded preexec.
# Emit order per spec §10.2:
#   PROMPT_COMMAND: capture $? first -> D;<code> -> A -> OSC 7 ; B lives at end of PS1 (\[ \]).
#   preexec/DEBUG : C exactly once.

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
}
PROMPT_COMMAND=_bpa_prompt

# --- embed B at the END of PS1, wrapped in \[ \] so bash does not miscount -
PS1="${PS1}\[$(_bpa_osc133 B)\]"

# --- preexec: bash-preexec if present, else a guarded DEBUG trap ----------
_bpa_preexec_ran=""
_bpa_preexec() {
  # Fire C exactly once per command; never for PROMPT_COMMAND itself.
  if [ -n "$COMP_LINE" ]; then return; fi         # skip completion
  if [ "$BASH_COMMAND" = "$PROMPT_COMMAND" ]; then return; fi
  if [ -n "$_bpa_preexec_ran" ]; then return; fi
  _bpa_preexec_ran=1
  _bpa_osc133 C
}

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
