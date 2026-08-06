# Bash records call arguments in BASH_ARGC/BASH_ARGV only under `extdebug`,
# and enabling it while the shell is still starting up — which is when
# BASH_ENV is read — means "start the bash debugger" instead. So arm it one
# command later, from a trap that removes itself before anything else can see
# it. The subject has no DEBUG trap of its own yet: BASH_ENV runs first.
#
# Returning zero is not optional: under extdebug a DEBUG handler that returns
# non-zero makes bash skip the command it fired for.
__bc_arm() { trap - DEBUG; shopt -s extdebug; builtin :; }

trap '__bc_arm' DEBUG
