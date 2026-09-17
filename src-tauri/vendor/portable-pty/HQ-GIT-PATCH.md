# HQ Git patch to portable-pty 0.9.0

Upstream: https://github.com/wezterm/wezterm/tree/portable-pty-0.9.0/pty
Original MIT license is preserved in LICENSE.md.

This copy adds an optional Windows Job handle to CommandBuilder and supplies it
through PROC_THREAD_ATTRIBUTE_JOB_LIST at CreateProcessW. This puts the Git
process and all descendants in the owning job before any user code executes.
The caller must keep the job handle alive until spawn_command returns.

ConPTY cursor inheritance is disabled: each embedded terminal starts empty, and
waiting for a cursor-position response would block startup before user input.

No shell is added. Command line quoting and ConPTY ownership remain upstream.

Unused example targets are omitted from this application dependency copy.
