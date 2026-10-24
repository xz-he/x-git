# Bundled SVN runtime

This directory is populated by `scripts/prepare-svn-runtime.ps1` during a
Windows package build. The generated files contain `bin/svn.exe`, its runtime
DLLs, and the redistributor license files. Do not commit the generated runtime
files to the repository.
